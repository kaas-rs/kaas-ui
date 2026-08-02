//! What a long-lived stream needs and a request/response handler does not.
//!
//! Two things live here, and both exist because a stream outlives the request
//! that started it:
//!
//! * a **drop-oldest hand-off** between the scan and the SSE writer, so a slow
//!   reader loses old records instead of stalling the fetch loop, and
//! * a **governor** counting how many streams are open, so one browser tab
//!   left open on a laptop lid cannot occupy the process,
//! * a **shutdown latch**, because a stream that never ends is a connection
//!   that never completes, and a server draining its connections would wait
//!   for it forever.

use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use tokio::sync::{Notify, watch};

// ---------------------------------------------------------------------------
// The hand-off
// ---------------------------------------------------------------------------

/// The shared state of one [`Sender`]/[`Receiver`] pair.
#[derive(Debug)]
struct Shared<T> {
    slots: Mutex<Slots<T>>,
    /// Signalled when an item is pushed, or when the sender goes away.
    ready: Notify,
    /// Signalled when the receiver goes away.
    closed: Notify,
}

#[derive(Debug)]
struct Slots<T> {
    queue: VecDeque<T>,
    capacity: usize,
    dropped: u64,
    sender_gone: bool,
    receiver_gone: bool,
}

impl<T> Shared<T> {
    /// The queue, even if a previous holder panicked while inside it.
    ///
    /// Panicking is denied at the workspace root, so poisoning should be
    /// unreachable — but `unwrap` is denied too, and recovering the inner
    /// value is both panic-free and the right answer: a poisoned queue of
    /// records is still a queue of records.
    fn slots(&self) -> MutexGuard<'_, Slots<T>> {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The producing half.
#[derive(Debug)]
pub struct Sender<T> {
    shared: Arc<Shared<T>>,
}

/// The consuming half.
#[derive(Debug)]
pub struct Receiver<T> {
    shared: Arc<Shared<T>>,
}

/// A bounded queue that drops its **oldest** entry when it overflows.
///
/// Drop-oldest rather than drop-newest, and never blocking, are the same
/// decision seen from two sides. A live tail whose reader has fallen behind
/// wants the newest records — showing someone a stale prefix of a topic that
/// has moved on is worse than showing them a gap — and awaiting a full queue
/// would push back through the SSE writer into the fetch loop, so one slow
/// browser would slow the scan for the cluster.
pub fn ring<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        slots: Mutex::new(Slots {
            queue: VecDeque::new(),
            capacity: capacity.max(1),
            dropped: 0,
            sender_gone: false,
            receiver_gone: false,
        }),
        ready: Notify::new(),
        closed: Notify::new(),
    });
    (
        Sender {
            shared: Arc::clone(&shared),
        },
        Receiver { shared },
    )
}

impl<T> Sender<T> {
    /// Queue an item, evicting the oldest if that is what it takes.
    ///
    /// Returns how many were evicted by *this* push, so a caller can attribute
    /// the loss to the moment it happened rather than to the end of the run.
    pub fn push(&self, item: T) -> u64 {
        let evicted = {
            let mut slots = self.shared.slots();
            if slots.receiver_gone {
                return 0;
            }
            slots.queue.push_back(item);
            let mut evicted = 0;
            while slots.queue.len() > slots.capacity {
                if slots.queue.pop_front().is_none() {
                    break;
                }
                evicted += 1;
            }
            slots.dropped += evicted;
            evicted
        };
        // `notify_one` stores a permit when nobody is waiting, so a receiver
        // that is between checks cannot miss this.
        self.shared.ready.notify_one();
        evicted
    }

    /// Whether the reader has gone away.
    pub fn is_closed(&self) -> bool {
        self.shared.slots().receiver_gone
    }

    /// Resolves when the reader goes away.
    ///
    /// This is what keeps the producing task from outliving its response: the
    /// pump selects on it, so a closed browser tab drops the scan rather than
    /// leaving it fetching into a queue nobody reads.
    pub async fn closed(&self) {
        loop {
            let notified = self.shared.closed.notified();
            if self.shared.slots().receiver_gone {
                return;
            }
            notified.await;
        }
    }
}

impl<T> Receiver<T> {
    /// The next item, or `None` once the sender is gone and the queue is dry.
    pub async fn recv(&mut self) -> Option<T> {
        loop {
            let notified = self.shared.ready.notified();
            {
                let mut slots = self.shared.slots();
                if let Some(item) = slots.queue.pop_front() {
                    return Some(item);
                }
                if slots.sender_gone {
                    return None;
                }
            }
            notified.await;
        }
    }

    /// How many items have been dropped to make room, over the whole stream.
    pub fn dropped(&self) -> u64 {
        self.shared.slots().dropped
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        self.shared.slots().sender_gone = true;
        self.shared.ready.notify_one();
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        self.shared.slots().receiver_gone = true;
        self.shared.closed.notify_one();
    }
}

// ---------------------------------------------------------------------------
// The governor
// ---------------------------------------------------------------------------

/// How many streams one caller may hold open at once.
const MAX_PER_PRINCIPAL: usize = 5;
/// How many the process will serve in total.
const MAX_TOTAL: usize = 50;

/// The open-stream count, per caller and overall.
#[derive(Debug, Default)]
pub struct StreamGovernor {
    state: Mutex<GovernorState>,
}

#[derive(Debug, Default)]
struct GovernorState {
    total: usize,
    next_id: u64,
    /// Per caller, oldest first, so the one to evict is the one at the front.
    per_principal: BTreeMap<String, VecDeque<Live>>,
}

/// One stream the governor is still counting.
#[derive(Debug)]
struct Live {
    id: u64,
    stop: ShutdownSignal,
}

/// Why a stream was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// This caller already holds the most it may.
    Caller {
        /// The ceiling they hit.
        limit: usize,
    },
    /// The process is at its ceiling.
    Process {
        /// The ceiling it hit.
        limit: usize,
    },
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Caller { limit } => write!(
                f,
                "you already have {limit} message streams open, which is the limit; \
                 close one before opening another"
            ),
            Self::Process { limit } => write!(
                f,
                "this kaas-ui is serving {limit} message streams, which is its limit; \
                 try again shortly"
            ),
        }
    }
}

impl StreamGovernor {
    /// Take a slot for a caller, closing their oldest stream if they are at
    /// their ceiling.
    ///
    /// **Evicting rather than refusing**, and that is the important decision.
    /// A ceiling only frees itself when the server notices a reader has gone,
    /// and behind a proxy it does not: code-server holds its upstream
    /// connection open after the browser has closed, so an abandoned stream
    /// keeps its slot until the lifetime cap expires half an hour later. Five
    /// page reloads then lock someone out of their own tool, with
    /// `EventSource` retrying into a wall.
    ///
    /// Eviction removes that failure entirely and bounds the resource just as
    /// well — a caller still holds at most [`MAX_PER_PRINCIPAL`] at once. It
    /// is also the better reading of intent: a browser asking for a sixth
    /// stream has almost certainly abandoned the first, and the newest request
    /// is the one a person is actually looking at.
    ///
    /// The per-caller ceiling applies only to a [`Principal`] that names one.
    /// Where nothing distinguishes callers, the process ceiling is the only
    /// honest bound — evicting a stranger's stream because someone else
    /// reconnected would be worse than either alternative.
    pub fn acquire(self: &Arc<Self>, principal: &Principal) -> Result<StreamPermit, Refusal> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        // The hard bound, and the only refusal left.
        if state.total >= MAX_TOTAL {
            return Err(Refusal::Process { limit: MAX_TOTAL });
        }

        let id = state.next_id;
        state.next_id = state.next_id.wrapping_add(1);
        let (stop, evicted) = shutdown_latch();

        let entry = state
            .per_principal
            .entry(principal.key.clone())
            .or_default();
        if principal.distinguishable && entry.len() >= MAX_PER_PRINCIPAL {
            // Removed here rather than left for the evicted stream's own
            // permit to drop: the count has to be right *now*, or this
            // acquisition would still see the caller at their ceiling.
            if let Some(oldest) = entry.pop_front() {
                oldest.stop.stop();
                state.total = state.total.saturating_sub(1);
            }
        }
        state
            .per_principal
            .entry(principal.key.clone())
            .or_default()
            .push_back(Live { id, stop });
        state.total += 1;

        Ok(StreamPermit {
            governor: Arc::clone(self),
            principal: principal.key.clone(),
            id,
            evicted,
        })
    }

    /// How many streams are open, for the acceptance run.
    pub fn open(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .total
    }
}

/// One held stream slot.
#[derive(Debug)]
pub struct StreamPermit {
    governor: Arc<StreamGovernor>,
    principal: String,
    id: u64,
    evicted: Shutdown,
}

impl StreamPermit {
    /// Fires when this stream is closed to make room for the same caller's
    /// newer one. The pump watches it exactly as it watches a shutdown.
    pub fn evicted(&self) -> Shutdown {
        self.evicted.clone()
    }
}

impl Drop for StreamPermit {
    fn drop(&mut self) {
        let mut state = self
            .governor
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        let Some(entry) = state.per_principal.get_mut(&self.principal) else {
            return;
        };
        // By id, because this permit may already have been evicted — and
        // decrementing twice for one stream would let the count drift below
        // reality until the ceiling stopped meaning anything.
        let Some(position) = entry.iter().position(|live| live.id == self.id) else {
            return;
        };
        entry.remove(position);
        let empty = entry.is_empty();
        state.total = state.total.saturating_sub(1);
        if empty {
            // The last one out removes the key, so the map does not grow by
            // one entry per caller the process has ever seen.
            state.per_principal.remove(&self.principal);
        }
    }
}

// ---------------------------------------------------------------------------
// Who is asking
// ---------------------------------------------------------------------------

/// Who a stream is charged to, and whether that actually names anybody.
///
/// There is no authentication yet — Phase 4 is where a user appears — so the
/// key is the nearest honest stand-in: the first `X-Forwarded-For` hop.
/// **It is a resource-accounting key, not a security boundary.**
///
/// `distinguishable` is the important half, and it is the difference between a
/// per-caller ceiling that protects people from each other and one that locks
/// them all out together. Without a forwarded header the peer address names
/// **the proxy**, not the caller: code-server's port proxy forwards no
/// `X-Forwarded-For` and rewrites `Host` to its own, so every browser behind it
/// arrives identically. Enforcing a per-caller ceiling on a key that does not
/// identify a caller means one person's reconnects refuse everyone's streams —
/// which is exactly what happened, and is strictly worse than not enforcing it
/// at all. You cannot be fair between callers you cannot tell apart.
#[derive(Debug, Clone)]
pub struct Principal {
    /// The accounting key.
    pub key: String,
    /// Whether it names one caller rather than whatever is in front of them.
    pub distinguishable: bool,
}

impl<S> FromRequestParts<S> for Principal
where
    S: Send + Sync,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        // A forwarded hop is the only thing here that names a caller.
        if let Some(first) = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|hop| !hop.is_empty())
        {
            return Ok(Self {
                key: first.to_owned(),
                distinguishable: true,
            });
        }
        // Still worth recording — it is what the logs and any future
        // per-source accounting want — but it is not an identity to ration by.
        if let Some(ConnectInfo(peer)) = parts.extensions.get::<ConnectInfo<SocketAddr>>() {
            return Ok(Self {
                key: peer.ip().to_string(),
                distinguishable: false,
            });
        }
        Ok(Self {
            key: "anonymous".to_owned(),
            distinguishable: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_oldest_entry_is_the_one_that_goes() {
        let (tx, mut rx) = ring::<i32>(3);
        for value in 1..=3 {
            assert_eq!(tx.push(value), 0);
        }
        // Over capacity: 1 leaves, not 4.
        assert_eq!(tx.push(4), 1);
        assert_eq!(rx.dropped(), 1);
        assert_eq!(rx.recv().await, Some(2));
        assert_eq!(rx.recv().await, Some(3));
        assert_eq!(rx.recv().await, Some(4));
    }

    #[tokio::test]
    async fn a_push_never_waits_for_a_reader() {
        // The property the whole type exists for: a reader that never reads
        // must not be able to slow a writer down.
        let (tx, _rx) = ring::<i32>(2);
        for value in 0..10_000 {
            tx.push(value);
        }
        assert!(!tx.is_closed());
    }

    #[tokio::test]
    async fn a_dropped_receiver_closes_the_sender() {
        let (tx, rx) = ring::<i32>(2);
        assert!(!tx.is_closed());
        drop(rx);
        assert!(tx.is_closed());
        // And the pump's cancellation point resolves rather than hanging.
        tokio::time::timeout(std::time::Duration::from_secs(1), tx.closed())
            .await
            .expect("closed() must resolve once the reader is gone");
    }

    #[tokio::test]
    async fn a_dropped_sender_ends_the_receiver_after_it_drains() {
        let (tx, mut rx) = ring::<i32>(4);
        tx.push(7);
        drop(tx);
        // Queued items still come out — a finished scan's last batch is not
        // forfeit because the producer returned.
        assert_eq!(rx.recv().await, Some(7));
        assert_eq!(rx.recv().await, None);
    }

    #[tokio::test]
    async fn a_receiver_waiting_on_an_empty_ring_is_woken_by_a_push() {
        let (tx, mut rx) = ring::<i32>(4);
        let waiting = tokio::spawn(async move { rx.recv().await });
        tokio::task::yield_now().await;
        tx.push(11);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
                .await
                .expect("the push must wake the reader")
                .expect("task"),
            Some(11)
        );
    }

    /// A caller a forwarded header actually named.
    fn named(key: &str) -> Principal {
        Principal {
            key: key.to_owned(),
            distinguishable: true,
        }
    }

    /// Everyone behind a proxy that forwards nothing — one key, many people.
    fn behind_a_proxy() -> Principal {
        Principal {
            key: "127.0.0.1".to_owned(),
            distinguishable: false,
        }
    }

    #[tokio::test]
    async fn a_named_caller_at_their_ceiling_loses_their_oldest_not_their_newest() {
        // The bug this replaced: refusing the newest meant a browser whose
        // abandoned streams could not be detected — two proxies deep, neither
        // propagating the disconnect — locked itself out of its own tool for
        // the half hour until the lifetime cap expired.
        let governor = Arc::new(StreamGovernor::default());
        let mut held = Vec::new();
        for _ in 0..MAX_PER_PRINCIPAL {
            held.push(
                governor
                    .acquire(&named("10.0.0.1"))
                    .expect("under the ceiling"),
            );
        }
        let oldest = held.remove(0);
        assert!(!oldest.evicted().is_stopping());

        let newest = governor
            .acquire(&named("10.0.0.1"))
            .expect("the newest request is the one a person is looking at");

        assert!(
            oldest.evicted().is_stopping(),
            "the oldest stream must be told to finish"
        );
        assert!(!newest.evicted().is_stopping());
        assert_eq!(
            governor.open(),
            MAX_PER_PRINCIPAL,
            "the ceiling still bounds the caller"
        );
    }

    #[tokio::test]
    async fn an_evicted_permit_does_not_decrement_twice() {
        // The evicted stream's pump ends and drops its permit some time after
        // the governor already removed it. Counting that twice would drift the
        // total below reality until the ceiling stopped meaning anything.
        let governor = Arc::new(StreamGovernor::default());
        let mut held = Vec::new();
        for _ in 0..MAX_PER_PRINCIPAL {
            held.push(
                governor
                    .acquire(&named("10.0.0.1"))
                    .expect("under the ceiling"),
            );
        }
        let newest = governor
            .acquire(&named("10.0.0.1"))
            .expect("evicts the oldest");
        assert_eq!(governor.open(), MAX_PER_PRINCIPAL);

        // The evicted permit drops late, as it would in the pump.
        let evicted = held.remove(0);
        drop(evicted);
        assert_eq!(
            governor.open(),
            MAX_PER_PRINCIPAL,
            "a late drop of an already-evicted permit must not double count"
        );

        drop(newest);
        drop(held);
        assert_eq!(governor.open(), 0, "and everything still releases");
    }

    #[test]
    fn one_caller_at_their_ceiling_does_not_touch_another() {
        let governor = Arc::new(StreamGovernor::default());
        let mut held = Vec::new();
        for _ in 0..MAX_PER_PRINCIPAL {
            held.push(
                governor
                    .acquire(&named("10.0.0.1"))
                    .expect("under the ceiling"),
            );
        }
        let other = governor
            .acquire(&named("10.0.0.2"))
            .expect("a different caller");
        let _ = governor
            .acquire(&named("10.0.0.1"))
            .expect("evicts its own oldest");
        assert!(
            !other.evicted().is_stopping(),
            "somebody else's stream must never be the one that makes room"
        );
    }

    #[test]
    fn callers_that_share_a_key_are_neither_capped_nor_evicted() {
        // Behind code-server every browser arrives identically. Rationing on
        // that key refuses — or now, evicts — a stranger's stream because
        // someone else reconnected, which is worse than not rationing at all.
        let governor = Arc::new(StreamGovernor::default());
        let mut held = Vec::new();
        for _ in 0..(MAX_PER_PRINCIPAL * 3) {
            held.push(
                governor
                    .acquire(&behind_a_proxy())
                    .expect("an unidentifiable caller must not hit a per-caller ceiling"),
            );
        }
        assert_eq!(governor.open(), MAX_PER_PRINCIPAL * 3);
        assert!(
            held.iter().all(|permit| !permit.evicted().is_stopping()),
            "no stream may be evicted for a caller that names nobody"
        );
    }

    #[test]
    fn the_process_ceiling_still_holds_for_callers_that_share_a_key() {
        // Dropping the per-caller ceiling must not drop the one that actually
        // protects the process.
        let governor = Arc::new(StreamGovernor::default());
        let mut held = Vec::new();
        for _ in 0..MAX_TOTAL {
            held.push(
                governor
                    .acquire(&behind_a_proxy())
                    .expect("under the total"),
            );
        }
        assert_eq!(
            governor.acquire(&behind_a_proxy()).err(),
            Some(Refusal::Process { limit: MAX_TOTAL })
        );
    }

    #[test]
    fn a_permit_releases_by_being_dropped() {
        let governor = Arc::new(StreamGovernor::default());
        {
            let _permit = governor.acquire(&named("10.0.0.1")).expect("acquired");
            assert_eq!(governor.open(), 1);
        }
        assert_eq!(governor.open(), 0);
        // And the map does not keep an entry per address ever seen.
        assert!(
            governor
                .state
                .lock()
                .map(|state| state.per_principal.is_empty())
                .unwrap_or(false)
        );
    }

    #[test]
    fn the_process_ceiling_holds_across_callers() {
        let governor = Arc::new(StreamGovernor::default());
        let mut held = Vec::new();
        for caller in 0..(MAX_TOTAL / MAX_PER_PRINCIPAL) {
            for _ in 0..MAX_PER_PRINCIPAL {
                held.push(
                    governor
                        .acquire(&named(&format!("10.0.0.{caller}")))
                        .expect("under both limits"),
                );
            }
        }
        assert_eq!(governor.open(), MAX_TOTAL);
        assert_eq!(
            governor.acquire(&named("10.0.1.1")).err(),
            Some(Refusal::Process { limit: MAX_TOTAL })
        );
    }
}

// ---------------------------------------------------------------------------
// The shutdown latch
// ---------------------------------------------------------------------------

/// Told once, remembered forever: stop streaming.
///
/// A graceful shutdown stops accepting new connections and then waits for the
/// in-flight ones to finish. An SSE response is an **unbounded body** — it
/// finishes when the stream does, and a live tail's stream finishes when the
/// client goes away or its lifetime expires. A shutdown is neither, so without
/// this the server waits for a response that will never complete and the
/// process has to be killed.
///
/// Kubernetes makes that concrete: SIGTERM, then `terminationGracePeriodSeconds`,
/// then SIGKILL. A stream severed by SIGKILL closes with no `phase: done`, so
/// the browser cannot tell "the server went away" from "the network broke", and
/// the dropped-record count that would have explained the gap dies with the
/// process.
///
/// Latching rather than a one-shot notification: a stream opened after the
/// signal — the window between the latch and the listener closing — must also
/// stop, and a `Notify` sent before it subscribed would be missed entirely.
#[derive(Debug, Clone)]
pub struct Shutdown {
    rx: watch::Receiver<bool>,
    /// Holds the channel open for a latch that has no owner — see [`Default`].
    ///
    /// Without it the sender drops immediately, and a dropped `watch::Sender`
    /// makes every waiter resolve at once: a latch documented as never firing
    /// would end every stream the moment it was awaited.
    keepalive: Option<Arc<ShutdownSignal>>,
}

/// The other half, held by whatever owns the process lifetime.
#[derive(Debug)]
pub struct ShutdownSignal {
    tx: watch::Sender<bool>,
}

/// A latch and the handle that trips it.
pub fn shutdown_latch() -> (ShutdownSignal, Shutdown) {
    let (tx, rx) = watch::channel(false);
    (
        ShutdownSignal { tx },
        Shutdown {
            rx,
            keepalive: None,
        },
    )
}

impl ShutdownSignal {
    /// Tell every open stream to finish.
    pub fn stop(&self) {
        // The error case is "nobody is listening", which is exactly the state
        // where there is nothing to tell.
        let _ = self.tx.send(true);
    }
}

impl Shutdown {
    /// Resolves when the process is shutting down, immediately if it already is.
    ///
    /// `wait_for` and not `changed`: `changed` waits for the *next* change, so
    /// a stream that opened after the latch tripped would hang on precisely the
    /// signal meant to release it.
    pub async fn wait(&self) {
        let mut rx = self.rx.clone();
        // An error means the sender is gone. Whatever owned the process
        // lifetime has dropped without saying stop, and treating that as
        // "stop" is the fail-safe reading: the worst it can do is end a stream
        // early and cleanly, where the opposite reading hangs the drain.
        let _ = rx.wait_for(|stopping| *stopping).await;
    }

    /// Whether the latch has already tripped.
    pub fn is_stopping(&self) -> bool {
        *self.rx.borrow()
    }
}

impl Default for Shutdown {
    /// A latch nobody can trip, for tests and for any caller that has no
    /// process lifetime to speak of.
    ///
    /// It keeps its own sender alive rather than dropping it, because a
    /// dropped sender means "stop" — see [`Shutdown::wait`].
    fn default() -> Self {
        let (signal, mut shutdown) = shutdown_latch();
        shutdown.keepalive = Some(Arc::new(signal));
        shutdown
    }
}

#[cfg(test)]
mod shutdown_tests {
    use super::*;

    #[tokio::test]
    async fn a_waiting_stream_is_released() {
        let (signal, shutdown) = shutdown_latch();
        assert!(!shutdown.is_stopping());

        let waiting = tokio::spawn(async move { shutdown.wait().await });
        tokio::task::yield_now().await;
        signal.stop();

        tokio::time::timeout(std::time::Duration::from_secs(1), waiting)
            .await
            .expect("stop() must release a waiting stream")
            .expect("task");
    }

    #[tokio::test]
    async fn a_stream_that_arrives_late_is_released_at_once() {
        // The window between the latch tripping and the listener closing. A
        // `Notify` would have been sent before this receiver existed, and this
        // stream would then have hung on the signal meant to end it.
        let (signal, shutdown) = shutdown_latch();
        signal.stop();
        assert!(shutdown.is_stopping());

        tokio::time::timeout(std::time::Duration::from_secs(1), shutdown.wait())
            .await
            .expect("an already-tripped latch must not block");
    }

    #[tokio::test]
    async fn a_sender_that_vanishes_stops_the_streams() {
        // Fail-safe. If whatever owned the process lifetime went away without
        // saying stop, ending the stream is the harmless reading; waiting
        // forever is the one that hangs a drain.
        let (signal, shutdown) = shutdown_latch();
        drop(signal);
        tokio::time::timeout(std::time::Duration::from_secs(1), shutdown.wait())
            .await
            .expect("a vanished sender must release, not hang");
    }

    #[tokio::test]
    async fn a_latch_nobody_trips_never_fires() {
        // The default, and what a stream must see for its whole normal life.
        let shutdown = Shutdown::default();
        assert!(!shutdown.is_stopping());
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(200), shutdown.wait())
                .await
                .is_err(),
            "an untripped latch must not release a stream"
        );
    }

    #[tokio::test]
    async fn every_clone_sees_one_signal() {
        // One signal, many open streams.
        let (signal, shutdown) = shutdown_latch();
        let waiters: Vec<_> = (0..8)
            .map(|_| {
                let shutdown = shutdown.clone();
                tokio::spawn(async move { shutdown.wait().await })
            })
            .collect();
        tokio::task::yield_now().await;
        signal.stop();
        for waiter in waiters {
            tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
                .await
                .expect("every open stream must be released")
                .expect("task");
        }
    }
}
