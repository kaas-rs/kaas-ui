//! The one file in this crate that can change a Connect cluster.
//!
//! [`ConnectWriter`]'s only constructor, [`ConnectClient::writer`], takes a
//! [`Grant`], and a `Grant` is minted only by the gate in `kaas-ui-auth` —
//! after the Connect cluster's `writes.allow`, a role, and Git ownership have
//! all agreed. Every method then checks that the grant is for *this*
//! operation on *this* connector, so a grant to restart `a` cannot stop `b`
//! or restart `c`. A mismatch is a kaas-ui bug and fails as one.
//!
//! `cargo xtask ci` lets a non-GET be spelled here and nowhere else in the
//! crate.

use kaas_ui_auth::{Grant, Operation};

use crate::client::{ConnectClient, segment};
use crate::fault::ConnectFault;

/// Writes to one connector, as one grant authorized.
#[derive(Debug)]
pub struct ConnectWriter<'a> {
    client: &'a ConnectClient,
    grant: &'a Grant,
}

impl ConnectClient {
    /// The writer for a granted operation on this Connect cluster.
    ///
    /// # Errors
    ///
    /// [`ConnectFault::Rejected`] when the grant names a different Connect
    /// cluster — which only a kaas-ui bug can produce.
    pub fn writer<'a>(&'a self, grant: &'a Grant) -> Result<ConnectWriter<'a>, ConnectFault> {
        if grant.target().host != self.id() {
            return Err(ConnectFault::Rejected(format!(
                "a grant for {:?} was presented to Connect cluster {:?}",
                grant.target().host,
                self.id()
            )));
        }
        Ok(ConnectWriter {
            client: self,
            grant,
        })
    }
}

impl ConnectWriter<'_> {
    /// `POST /connectors/{c}/restart?includeTasks=&onlyFailed=`.
    ///
    /// With both flags false Connect restarts the connector instance alone;
    /// `includeTasks` adds its tasks, and `onlyFailed` narrows either to the
    /// ones that failed — which, on a healthy connector, restarts nothing.
    pub async fn restart(
        &self,
        include_tasks: bool,
        only_failed: bool,
    ) -> Result<(), ConnectFault> {
        let name = self.connector(Operation::ConnectorRestart)?;
        self.send(
            reqwest::Method::POST,
            &format!(
                "/connectors/{}/restart?includeTasks={include_tasks}&onlyFailed={only_failed}",
                segment(name)
            ),
        )
        .await
    }

    /// `PUT /connectors/{c}/pause`.
    pub async fn pause(&self) -> Result<(), ConnectFault> {
        let name = self.connector(Operation::ConnectorPause)?;
        self.send(
            reqwest::Method::PUT,
            &format!("/connectors/{}/pause", segment(name)),
        )
        .await
    }

    /// `PUT /connectors/{c}/resume`.
    pub async fn resume(&self) -> Result<(), ConnectFault> {
        let name = self.connector(Operation::ConnectorResume)?;
        self.send(
            reqwest::Method::PUT,
            &format!("/connectors/{}/resume", segment(name)),
        )
        .await
    }

    /// `PUT /connectors/{c}/stop`.
    pub async fn stop(&self) -> Result<(), ConnectFault> {
        let name = self.connector(Operation::ConnectorStop)?;
        self.send(
            reqwest::Method::PUT,
            &format!("/connectors/{}/stop", segment(name)),
        )
        .await
    }

    /// The connector this grant is for, if it is for `expected`.
    fn connector(&self, expected: Operation) -> Result<&str, ConnectFault> {
        if self.grant.operation() != expected {
            return Err(ConnectFault::Rejected(format!(
                "a grant for {} was used to {}",
                self.grant.operation().name(),
                expected.name()
            )));
        }
        self.grant.target().name.as_deref().ok_or_else(|| {
            ConnectFault::Rejected(format!(
                "a grant for {} names no connector",
                expected.name()
            ))
        })
    }

    /// **The only mutating request in this crate.**
    async fn send(&self, method: reqwest::Method, path: &str) -> Result<(), ConnectFault> {
        let client = self.client;
        let request = client.authenticate(
            client
                .http()
                .request(method, client.url_for(path))
                .header(reqwest::header::ACCEPT, "application/json"),
        );
        client.count();
        let response = request
            .send()
            .await
            .map_err(|error| client.unreachable(&error))?;
        // Whatever happened, the cached list may now be wrong.
        client.invalidate();
        if !response.status().is_success() {
            return Err(client.fault_of(response, path).await);
        }
        Ok(())
    }
}
