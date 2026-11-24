use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct JobTicket {
    pub id: String,
    pub stop: CancellationToken,
}

#[derive(Default)]
pub struct JobRegistry {
    sequence: AtomicU64,
    lanes: Mutex<HashMap<String, JobTicket>>,
}

impl JobRegistry {
    pub fn begin(&self, lane: &str) -> JobTicket {
        let ticket = JobTicket {
            id: self.sequence.fetch_add(1, Ordering::Relaxed).to_string(),
            stop: CancellationToken::new(),
        };
        let mut lanes = self.lanes.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(previous) = lanes.insert(lane.into(), ticket.clone()) {
            previous.stop.cancel();
        }
        ticket
    }

    pub fn current(&self, lane: &str) -> Option<JobTicket> {
        self.lanes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(lane)
            .filter(|ticket| !ticket.stop.is_cancelled())
            .cloned()
    }

    pub fn is_current(&self, lane: &str, ticket: &JobTicket) -> bool {
        !ticket.stop.is_cancelled()
            && self
                .lanes
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(lane)
                .is_some_and(|current| current.id == ticket.id)
    }

    /// Marks a lane's job as done without cancelling it, so the lane reads as
    /// idle. Ignored if a newer job already owns the lane.
    pub fn finish(&self, lane: &str, ticket: &JobTicket) {
        let mut lanes = self.lanes.lock().unwrap_or_else(|e| e.into_inner());
        if lanes
            .get(lane)
            .is_some_and(|current| current.id == ticket.id)
        {
            lanes.remove(lane);
        }
    }

    /// True while a job that has neither finished nor been cancelled owns the lane.
    pub fn is_running(&self, lane: &str) -> bool {
        self.lanes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(lane)
            .is_some_and(|current| !current.stop.is_cancelled())
    }

    /// True while any lane starting with `prefix` has an active uncancelled job.
    pub fn has_running_prefix(&self, prefix: &str) -> bool {
        self.lanes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|(name, current)| name.starts_with(prefix) && !current.stop.is_cancelled())
    }

    pub fn cancel(&self, lane: &str) {
        if let Some(ticket) = self
            .lanes
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(lane)
        {
            ticket.stop.cancel();
        }
    }

    pub fn cancel_all(&self) {
        for (_, ticket) in self.lanes.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            ticket.stop.cancel();
        }
    }

    pub fn cancel_prefix(&self, prefix: &str) {
        let mut lanes = self.lanes.lock().unwrap_or_else(|e| e.into_inner());
        let names: Vec<_> = lanes
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect();
        for name in names {
            if let Some(ticket) = lanes.remove(&name) {
                ticket.stop.cancel();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_wins_without_cancelling_other_lanes() {
        let registry = JobRegistry::default();
        let text = registry.begin("text");
        let visual = registry.begin("visual");
        let newer = registry.begin("text");
        assert!(text.stop.is_cancelled());
        assert!(!registry.is_current("text", &text));
        assert!(registry.is_current("visual", &visual));
        assert!(registry.is_current("text", &newer));
        registry.cancel("text");
        assert!(!registry.is_current("text", &newer));
    }

    #[test]
    fn finishing_frees_the_lane_but_never_removes_a_newer_job() {
        let registry = JobRegistry::default();
        let first = registry.begin("visual");
        assert!(registry.is_running("visual"));
        let second = registry.begin("visual");
        registry.finish("visual", &first);
        assert!(registry.is_running("visual"));
        assert!(!second.stop.is_cancelled());
        registry.finish("visual", &second);
        assert!(!registry.is_running("visual"));
        assert!(!second.stop.is_cancelled());
        assert!(!registry.is_running("never-started"));
    }

    #[test]
    fn replacing_a_capture_cancels_block_jobs_without_cancelling_text() {
        let registry = JobRegistry::default();
        let text = registry.begin("text");
        let first = registry.begin("block:1:a");
        let second = registry.begin("block:1:b");
        registry.cancel_prefix("block:");
        assert!(first.stop.is_cancelled());
        assert!(second.stop.is_cancelled());
        assert!(registry.is_current("text", &text));
        assert!(registry.is_current("block:2:a", &registry.begin("block:2:a")));
    }
}
