//! The user JavaScript predicate: the only code kaas-ui runs that it did not
//! write.
//!
//! Three non-negotiables, and all three are properties of *construction*
//! rather than things a reviewer has to remember:
//!
//! 1. **A hard memory cap and an interrupt handler, both installed before the
//!    first evaluation** — including before the user's own source is compiled,
//!    because a pathological expression can hang a parser as easily as a loop
//!    can hang an interpreter. A predicate that allocates or loops forever is
//!    killed by the runtime, not by the pod's OOM killer taking every other
//!    cluster down with it.
//! 2. **No host bindings.** The context is built from QuickJS's own
//!    intrinsics, which are language, not I/O: `JSON`, `RegExp`, `Date`,
//!    `Math`. There is no `fetch`, no `fs`, no timer, and nothing is added —
//!    the predicate sees one argument and returns a boolean.
//! 3. **It never runs on a record a cheap filter could have dropped.**
//!    kaas-lib's `RecordFilter` — offset, timestamp, partition, key prefix,
//!    headers — is in the scan spec and runs before deserialization, and the
//!    caller's own floor check runs before the decode. This module cannot
//!    enforce that ordering; it can only make the cost of getting it wrong
//!    visible, which is what [`PredicateStats`] is for.
//!
//! The budget is **per record**. A predicate too slow for one record does not
//! stop the scan: that record is skipped and counted, so "my filter is too
//! slow" is a number on screen rather than a mystery.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rquickjs::{Context, Function, Runtime};
use serde::Serialize;
use utoipa::ToSchema;

/// The memory a predicate may hold.
///
/// Generous for an expression over one record and far below anything that
/// would trouble the process. The point is not to be exactly right; it is that
/// the ceiling exists and is enforced by the interpreter, so hitting it throws
/// an exception rather than ending the pod.
pub const MEMORY_LIMIT: usize = 16 * 1024 * 1024;

/// The stack a predicate may use, which is what bounds runaway recursion.
pub const STACK_LIMIT: usize = 256 * 1024;

/// How long one record's evaluation may take.
pub const RECORD_BUDGET: Duration = Duration::from_millis(10);

/// How long compiling the expression itself may take.
///
/// Longer than a record's budget and still bounded: this happens once, and a
/// source that cannot be parsed inside it is not one anybody meant to write.
const COMPILE_BUDGET: Duration = Duration::from_millis(250);

/// Why a predicate could not be compiled.
#[derive(Debug, thiserror::Error)]
pub enum PredicateError {
    /// The QuickJS runtime would not start.
    #[error("could not start the JavaScript runtime: {0}")]
    Runtime(String),
    /// The expression did not compile.
    #[error("the filter expression did not compile: {0}")]
    Compile(String),
}

/// What a predicate has done so far.
///
/// Reported beside the scan's own progress, because a filter that silently
/// dropped a thousand records for exceeding its budget looks exactly like a
/// filter that matched nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PredicateStats {
    /// Records the predicate was run on.
    pub evaluated: u64,
    /// Records it accepted.
    pub matched: u64,
    /// Records killed by the per-record budget. **Not** the same as rejected:
    /// nobody knows whether these matched.
    pub timed_out: u64,
    /// Records whose evaluation threw — a bad field access on one odd record,
    /// or the memory cap firing.
    pub failed: u64,
    /// The most recent failure, so one bad expression is diagnosable without
    /// a log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// A compiled user predicate, with its own interpreter.
///
/// One per scan. The runtime is not shared between streams — a predicate that
/// exhausts its memory must take nothing with it, and a shared interpreter is
/// exactly a thing to take.
pub struct Predicate {
    // Dropped after `context`, which is what the field order says. The
    // runtime outliving its context is the requirement, not the reverse.
    context: Context,
    _runtime: Runtime,
    /// When the evaluation in flight must stop. Read by the interrupt handler
    /// on the same thread that set it.
    deadline: Arc<Mutex<Option<Instant>>>,
    budget: Duration,
    evaluated: AtomicU64,
    matched: AtomicU64,
    timed_out: AtomicU64,
    failed: AtomicU64,
    last_error: Mutex<Option<String>>,
}

/// Hand-written because neither the runtime nor the context has a `Debug`,
/// and what a reader of a log wants from a predicate is its counters anyway.
impl std::fmt::Debug for Predicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Predicate")
            .field("budget", &self.budget)
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl Predicate {
    /// Compile a user expression.
    ///
    /// Either an arrow function — `v => v.amount > 100` — or a bare expression
    /// over `value`, which is what people type first. Both end up as the same
    /// one-argument function; which one was written is not a distinction worth
    /// making somebody learn.
    pub fn compile(source: &str) -> Result<Self, PredicateError> {
        Self::with_budget(source, RECORD_BUDGET)
    }

    /// The same, with a different per-record budget. For the tests.
    pub fn with_budget(source: &str, budget: Duration) -> Result<Self, PredicateError> {
        let runtime = Runtime::new().map_err(|error| PredicateError::Runtime(error.to_string()))?;

        // Before the context exists, and therefore before anything at all has
        // been evaluated — including the user's own source below.
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(STACK_LIMIT);

        let deadline: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let watched = Arc::clone(&deadline);
        runtime.set_interrupt_handler(Some(Box::new(move || match watched.lock() {
            Ok(deadline) => deadline.is_some_and(|at| Instant::now() >= at),
            // A poisoned lock means a thread died holding it. Interrupting is
            // the safe direction: the alternative is a loop nothing can stop.
            Err(_) => true,
        })));

        let context =
            Context::full(&runtime).map_err(|error| PredicateError::Runtime(error.to_string()))?;

        let predicate = Self {
            context,
            _runtime: runtime,
            deadline,
            budget,
            evaluated: AtomicU64::new(0),
            matched: AtomicU64::new(0),
            timed_out: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            last_error: Mutex::new(None),
        };

        predicate.arm(COMPILE_BUDGET);
        let result = predicate.context.with(|ctx| {
            ctx.eval::<(), _>(bootstrap(source))
                .map_err(|error| PredicateError::Compile(describe(&ctx, error)))
        });
        predicate.disarm();
        result?;

        Ok(predicate)
    }

    /// Whether one decoded value satisfies the predicate.
    ///
    /// A record whose evaluation was killed or threw is **excluded**, and
    /// counted. Including it would let a slow expression quietly turn into no
    /// expression; excluding it silently would be the same failure in the
    /// other direction, which is why nothing here is silent.
    pub fn matches(&self, value: &serde_json::Value) -> bool {
        // Serialised on this side rather than converted value by value: one
        // string crossing the boundary is cheaper than a tree of them, and
        // QuickJS's own `JSON.parse` is the fastest parser in the process.
        let json = match serde_json::to_string(value) {
            Ok(json) => json,
            Err(error) => {
                self.fail(&error.to_string());
                return false;
            }
        };

        self.evaluated.fetch_add(1, Ordering::Relaxed);
        self.arm(self.budget);
        let outcome = self.context.with(|ctx| {
            let entry: Function = ctx.globals().get("__kaas")?;
            entry.call::<_, bool>((json,))
        });
        let expired = self.expired();
        self.disarm();

        match outcome {
            Ok(true) => {
                self.matched.fetch_add(1, Ordering::Relaxed);
                true
            }
            Ok(false) => false,
            Err(error) => {
                // An interrupt and a thrown exception arrive the same way, and
                // they are not the same thing to a reader: one says "your
                // filter is too slow", the other says "your filter is wrong".
                if expired {
                    self.timed_out.fetch_add(1, Ordering::Relaxed);
                } else {
                    let message = self.context.with(|ctx| describe(&ctx, error));
                    self.fail(&message);
                }
                false
            }
        }
    }

    /// What this predicate has done so far.
    #[must_use]
    pub fn stats(&self) -> PredicateStats {
        PredicateStats {
            evaluated: self.evaluated.load(Ordering::Relaxed),
            matched: self.matched.load(Ordering::Relaxed),
            timed_out: self.timed_out.load(Ordering::Relaxed),
            failed: self.failed.load(Ordering::Relaxed),
            last_error: self.last_error.lock().ok().and_then(|error| error.clone()),
        }
    }

    fn arm(&self, budget: Duration) {
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = Instant::now().checked_add(budget);
        }
    }

    fn disarm(&self) {
        if let Ok(mut deadline) = self.deadline.lock() {
            *deadline = None;
        }
    }

    fn expired(&self) -> bool {
        match self.deadline.lock() {
            Ok(deadline) => deadline.is_some_and(|at| Instant::now() >= at),
            Err(_) => false,
        }
    }

    fn fail(&self, message: &str) {
        self.failed.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut last) = self.last_error.lock() {
            *last = Some(message.to_owned());
        }
    }
}

/// Turn a `rquickjs::Error` into the sentence the exception actually carried.
///
/// Without this every failure reads `Exception generated by quickjs`, which
/// tells a reader nothing about the expression they just typed.
fn describe(ctx: &rquickjs::Ctx<'_>, error: rquickjs::Error) -> String {
    if error.is_exception() {
        let exception = ctx.catch();
        if let Some(exception) = exception.as_exception() {
            let message = exception.message().unwrap_or_default();
            return if message.is_empty() {
                exception.to_string()
            } else {
                message
            };
        }
        // An exception that is not an `Error` — `throw "nope"` is legal.
        if let Some(text) = exception.as_string().and_then(|s| s.to_string().ok()) {
            return text;
        }
    }
    error.to_string()
}

/// The program that installs the predicate.
///
/// The source is embedded as a **JSON string literal**, never spliced in raw,
/// so nothing a user types can end the statement it is inside. It is then
/// turned into a function in the sandbox: `eval` and `Function` are language,
/// not host access, and the sandbox has no I/O to reach either way.
fn bootstrap(source: &str) -> String {
    // `to_string` on a `&str` cannot fail.
    let literal = serde_json::to_string(source).unwrap_or_else(|_| "\"false\"".to_owned());
    format!(
        r#"
globalThis.__kaas_source = {literal};
globalThis.__kaas_p = (function () {{
    const src = globalThis.__kaas_source;
    let f;
    // An arrow function or a `function` expression, which is the documented
    // shape.
    try {{ f = (0, eval)('(' + src + ')'); }} catch (e) {{ f = undefined; }}
    if (typeof f !== 'function') {{
        // A bare expression over `value`, which is what people type first.
        // A genuinely broken expression throws here, at compile time, rather
        // than once per record.
        f = new Function('value', 'return (' + src + ');');
    }}
    return f;
}})();
globalThis.__kaas = function (json) {{
    const result = globalThis.__kaas_p(JSON.parse(json));
    if (result !== null && (typeof result === 'object' || typeof result === 'function')) {{
        // A predicate that returns a promise or an object would be truthy for
        // every record, which is indistinguishable from no filter at all.
        throw new TypeError('the filter returned a ' + typeof result + '; it has to return a boolean');
    }}
    return !!result;
}};
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_arrow_function_filters() {
        let predicate = Predicate::compile("v => v.amount > 100").unwrap();
        assert!(predicate.matches(&json!({ "amount": 500 })));
        assert!(!predicate.matches(&json!({ "amount": 5 })));

        let stats = predicate.stats();
        assert_eq!(stats.evaluated, 2);
        assert_eq!(stats.matched, 1);
        assert_eq!(stats.timed_out, 0);
        assert_eq!(stats.failed, 0);
    }

    #[test]
    fn a_bare_expression_over_value_filters_too() {
        // What people type first, and refusing it would be a lesson nobody
        // asked for.
        let predicate = Predicate::compile("value.amount > 100").unwrap();
        assert!(predicate.matches(&json!({ "amount": 500 })));
        assert!(!predicate.matches(&json!({ "amount": 5 })));
    }

    #[test]
    fn a_text_payload_reaches_the_predicate_as_a_string() {
        let predicate =
            Predicate::compile("v => typeof v === 'string' && v.includes('boom')").unwrap();
        assert!(predicate.matches(&json!("something went boom")));
        assert!(!predicate.matches(&json!("all fine")));
    }

    #[test]
    fn a_tombstone_is_null_rather_than_absent() {
        let predicate = Predicate::compile("v => v === null").unwrap();
        assert!(predicate.matches(&serde_json::Value::Null));
        assert!(!predicate.matches(&json!({})));
    }

    #[test]
    fn an_expression_that_does_not_compile_is_refused_at_compile_time() {
        // Once, here — not once per record, and not as a filter that silently
        // matches everything.
        let error = Predicate::compile("v => v.amount >>> ((((").unwrap_err();
        assert!(matches!(error, PredicateError::Compile(_)), "{error}");
    }

    /// The non-negotiable: `while(true){}` is killed by the interrupt handler.
    #[test]
    fn an_infinite_loop_is_killed_within_the_budget() {
        let predicate =
            Predicate::with_budget("v => { while (true) {} }", Duration::from_millis(20)).unwrap();

        let started = Instant::now();
        assert!(!predicate.matches(&json!({ "a": 1 })));
        let took = started.elapsed();

        assert!(
            took < Duration::from_secs(2),
            "the interrupt handler did not fire: {took:?}"
        );
        let stats = predicate.stats();
        assert_eq!(stats.timed_out, 1, "{stats:?}");
        assert_eq!(stats.matched, 0);

        // And the runtime survives it. A budget that killed the *scan* rather
        // than the record would be a denial of service with extra steps.
        assert!(predicate.matches(&json!({ "a": 1 })) || predicate.stats().timed_out == 2);
    }

    /// The other non-negotiable: allocating in a loop hits the cap and throws,
    /// rather than the process being killed by the kernel.
    #[test]
    fn allocating_forever_hits_the_memory_cap_rather_than_the_oom_killer() {
        // A generous budget, so this fails for the reason it is meant to
        // rather than by running out of time first.
        let predicate = Predicate::with_budget(
            "v => { const a = []; for (;;) { a.push(new Array(100000).fill('x')); } }",
            Duration::from_secs(30),
        )
        .unwrap();

        assert!(!predicate.matches(&json!({ "a": 1 })));
        let stats = predicate.stats();
        assert_eq!(stats.evaluated, 1);
        assert_eq!(stats.matched, 0);
        assert_eq!(
            stats.timed_out + stats.failed,
            1,
            "the allocation was neither capped nor timed out: {stats:?}"
        );
    }

    #[test]
    fn there_are_no_host_bindings() {
        // Not an exhaustive proof — that is a property of what was *not*
        // added — but the four anybody would reach for first.
        for name in ["fetch", "setTimeout", "require", "process"] {
            let predicate =
                Predicate::compile(&format!("v => typeof {name} === 'undefined'")).unwrap();
            assert!(
                predicate.matches(&json!({})),
                "{name} is reachable from a user predicate"
            );
        }
    }

    #[test]
    fn a_predicate_that_returns_a_promise_is_an_error_rather_than_a_match() {
        // `!!promise` is `true`, so without the type check an `async` filter
        // would match every record and look like it worked.
        let predicate = Predicate::compile("async v => v.amount > 100").unwrap();
        assert!(!predicate.matches(&json!({ "amount": 500 })));
        let stats = predicate.stats();
        assert_eq!(stats.failed, 1);
        assert!(
            stats
                .last_error
                .as_ref()
                .is_some_and(|error| error.contains("boolean")),
            "{stats:?}"
        );
    }

    #[test]
    fn a_thrown_expression_is_reported_rather_than_swallowed() {
        let predicate = Predicate::compile("v => v.nothing.here").unwrap();
        assert!(!predicate.matches(&json!({})));
        let stats = predicate.stats();
        assert_eq!(stats.failed, 1);
        assert!(stats.last_error.is_some(), "{stats:?}");
    }

    /// The predicate has to be usable from the task a stream runs on.
    #[test]
    fn a_predicate_can_be_moved_to_another_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<Predicate>();
    }
}
