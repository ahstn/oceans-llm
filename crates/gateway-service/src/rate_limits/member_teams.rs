//! The team of a user whose credential carries none, for team-scoped
//! policies: looked up once per uncached user, cached for `policy_refresh`,
//! and carried from a request's check to its token charge.

use std::{collections::HashMap, sync::Arc, time::Instant};

use gateway_core::{RateLimitMetric, RateLimitScopeKind, StoreError};
use uuid::Uuid;

use super::{PolicySet, RateLimitSubject, RateLimiter};

/// Finds the team a user belongs to, so team-scoped policies also count the
/// requests members make with their own keys and OAuth grants.
#[async_trait::async_trait]
pub trait RateLimitTeamLookup: Send + Sync {
    async fn team_for_user(&self, user_id: Uuid) -> Result<Option<Uuid>, StoreError>;
}

#[async_trait::async_trait]
impl<T> RateLimitTeamLookup for T
where
    T: gateway_core::IdentityRepository + ?Sized,
{
    async fn team_for_user(&self, user_id: Uuid) -> Result<Option<Uuid>, StoreError> {
        Ok(self
            .get_team_membership_for_user(user_id)
            .await?
            .map(|membership| membership.team_id))
    }
}

/// A user's team as a check resolved it, for the same request's token charge
/// (see [`RateLimitSubject::with_member_team`]). The default is unresolved:
/// the check needed no team, or could not read it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateLimitMemberTeam(Option<Option<Uuid>>);

impl RateLimitMemberTeam {
    pub(super) fn of(subject: &RateLimitSubject<'_>) -> Self {
        Self(subject.team_resolved.then_some(subject.team_id))
    }
}

impl RateLimitSubject<'_> {
    /// Uses the team the request's check resolved, so its token charge counts
    /// the same team without reading it again. A request can outlast the
    /// cached team; a charge that then looked it up would not know its
    /// counters up front, and every token check on the replica would wait
    /// for it.
    #[must_use]
    pub fn with_member_team(mut self, team: RateLimitMemberTeam) -> Self {
        if let RateLimitMemberTeam(Some(team_id)) = team {
            self.team_id = team_id;
            self.team_resolved = true;
        }
        self
    }
}

/// One lookup's outcome, shared with every caller waiting on it. A failure
/// keeps its message, since `StoreError` cannot be cloned.
type TeamLookupOutcome = Result<Option<Uuid>, String>;
pub(super) type TeamLookupFlights = std::sync::Mutex<HashMap<Uuid, TeamLookupEntry>>;

/// A user's lookup in flight and how many callers share it.
#[derive(Default)]
pub(super) struct TeamLookupEntry {
    cell: Arc<tokio::sync::OnceCell<TeamLookupOutcome>>,
    callers: usize,
}

/// A caller's share of one user's team lookup. The entry stays while any
/// caller holds a share, so one waiter giving up cannot let a second,
/// overlapping lookup start. Dropping the last share, whether the lookup
/// succeeded, failed or every caller was cancelled, removes the entry.
struct TeamLookupFlight<'a> {
    flights: &'a TeamLookupFlights,
    user_id: Uuid,
    cell: Arc<tokio::sync::OnceCell<TeamLookupOutcome>>,
}

impl<'a> TeamLookupFlight<'a> {
    fn join(flights: &'a TeamLookupFlights, user_id: Uuid) -> Self {
        let mut map = flights.lock().unwrap_or_else(|poison| poison.into_inner());
        let entry = map.entry(user_id).or_default();
        entry.callers += 1;
        Self {
            flights,
            user_id,
            cell: entry.cell.clone(),
        }
    }
}

impl Drop for TeamLookupFlight<'_> {
    fn drop(&mut self) {
        let mut flights = self
            .flights
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let Some(entry) = flights.get_mut(&self.user_id) else {
            return;
        };
        if !Arc::ptr_eq(&entry.cell, &self.cell) {
            return;
        }
        entry.callers -= 1;
        if entry.callers == 0 {
            flights.remove(&self.user_id);
        }
    }
}

/// Users whose team is cached at once. Past this, expired entries are
/// dropped, and the cache starts over only if that frees nothing.
pub(super) const MAX_CACHED_MEMBER_TEAMS: usize = 10_000;

impl RateLimiter {
    /// Resolves the team of a user whose credential carries none, for
    /// team-scoped policies. Without it those policies count only team
    /// (service account) keys.
    #[must_use]
    pub fn with_team_lookup(mut self, lookup: Arc<dyn RateLimitTeamLookup>) -> Self {
        self.team_lookup = Some(lookup);
        self
    }

    /// The user's team from the cache, when it was read recently.
    pub(super) fn cached_member_team(&self, user_id: Uuid) -> Option<Option<Uuid>> {
        let teams = self
            .member_teams
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        teams
            .get(&user_id)
            .filter(|(read_at, _)| read_at.elapsed() < self.settings.policy_refresh)
            .map(|(_, team)| *team)
    }

    /// Whether `subject` needs its user's team looked up for `set`: some
    /// team policy, of `metric` when given, matches the request's traffic.
    pub(super) fn needs_member_team(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'_>,
        metric: Option<RateLimitMetric>,
    ) -> bool {
        self.team_lookup.is_some()
            && !subject.team_resolved
            && subject.team_id.is_none()
            && subject.user_id.is_some()
            && set.for_operation(subject.operation).iter().any(|policy| {
                policy.scope_kind == RateLimitScopeKind::Team
                    && metric.is_none_or(|metric| policy.metric == metric)
                    && policy.matches_traffic(subject)
            })
    }

    /// `subject` with its user's team filled in, when a team policy of
    /// `metric` (any, when `None`) matches the request. A failed lookup is an
    /// error, which callers handle like any other store failure.
    pub(super) async fn resolve_member_team<'a>(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'a>,
        metric: Option<RateLimitMetric>,
    ) -> Result<RateLimitSubject<'a>, StoreError> {
        let mut subject = *subject;
        if !self.needs_member_team(set, &subject, metric) {
            return Ok(subject);
        }
        let (Some(lookup), Some(user_id)) = (&self.team_lookup, subject.user_id) else {
            return Ok(subject);
        };
        if let Some(team) = self.cached_member_team(user_id) {
            subject.team_id = team;
            subject.team_resolved = true;
            return Ok(subject);
        }
        let flight = TeamLookupFlight::join(&self.member_team_lookups, user_id);
        // Callers already waiting share a failure too, rather than retrying
        // the lookup one after another; the next request starts a new one.
        let team = flight
            .cell
            .get_or_init(|| async {
                lookup
                    .team_for_user(user_id)
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .clone()
            .map_err(StoreError::Unavailable)?;
        self.cache_member_team(user_id, team);
        subject.team_id = team;
        subject.team_resolved = true;
        Ok(subject)
    }

    fn cache_member_team(&self, user_id: Uuid, team: Option<Uuid>) {
        let mut teams = self
            .member_teams
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if teams.len() >= MAX_CACHED_MEMBER_TEAMS {
            let refresh = self.settings.policy_refresh;
            teams.retain(|_, (read_at, _)| read_at.elapsed() < refresh);
            if teams.len() >= MAX_CACHED_MEMBER_TEAMS {
                teams.clear();
            }
        }
        teams.insert(user_id, (Instant::now(), team));
    }

    /// [`Self::resolve_member_team`] for a token charge. A failed lookup
    /// charges the other counters and marks token counters stale, as a failed
    /// usage write does, so `closed` mode refuses token checks it can no
    /// longer trust.
    pub(super) async fn charge_subject<'a>(
        &self,
        set: &PolicySet,
        subject: &RateLimitSubject<'a>,
    ) -> RateLimitSubject<'a> {
        match self
            .resolve_member_team(set, subject, Some(RateLimitMetric::Tokens))
            .await
        {
            Ok(subject) => subject,
            Err(error) => {
                tracing::warn!(error = %error, "could not read the user's team for a token charge");
                self.mark_token_write_failed();
                *subject
            }
        }
    }
}
