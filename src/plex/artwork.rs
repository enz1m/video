use iced::advanced::image::Id;
use iced::widget::image::{Allocation, Error, Handle};
use std::collections::{HashMap, HashSet};

const MAX_BACKDROP_BYTES: u64 = 32 * 1024 * 1024;

fn rgba_bytes(width: u32, height: u32) -> u64 {
    u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4)
}

#[derive(Default)]
pub(crate) struct BackdropCache {
    ready: HashMap<Id, Allocation>,
    pending: HashMap<Id, (u64, u64)>,
    failed: HashSet<Id>,
    serial: u64,
}

impl BackdropCache {
    pub(crate) fn reconcile(&mut self, handles: Vec<Handle>) -> Vec<(u64, Handle)> {
        let mut live = HashSet::new();
        let mut bytes = 0_u64;
        let mut selected = Vec::new();
        for handle in handles {
            let Handle::Rgba { width, height, .. } = &handle else {
                continue;
            };
            let size = rgba_bytes(*width, *height);
            if live.contains(&handle.id()) || size > MAX_BACKDROP_BYTES.saturating_sub(bytes) {
                continue;
            }
            bytes += size;
            live.insert(handle.id());
            selected.push((handle, size));
        }
        self.ready.retain(|id, _| live.contains(id));
        self.pending.retain(|id, _| live.contains(id));
        self.failed.retain(|id| live.contains(id));
        let mut requests = Vec::new();
        for (handle, size) in selected {
            let id = handle.id();
            if self.ready.contains_key(&id)
                || self.pending.contains_key(&id)
                || self.failed.contains(&id)
            {
                continue;
            }
            self.serial = self.serial.wrapping_add(1);
            self.pending.insert(id, (self.serial, size));
            requests.push((self.serial, handle));
        }
        requests
    }

    pub(crate) fn complete(&mut self, ticket: u64, id: Id, result: Result<Allocation, Error>) {
        let Some((expected, reserved)) = self.pending.get(&id).copied() else {
            return;
        };
        if ticket != expected {
            return;
        }
        self.pending.remove(&id);
        match result {
            Ok(allocation)
                if allocation.handle().id() == id
                    && rgba_bytes(allocation.size().width, allocation.size().height)
                        <= reserved =>
            {
                // Retained allocations prevent asynchronous reuploads when offscreen artwork returns.
                self.ready.insert(id, allocation);
            }
            _ => {
                self.failed.insert(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backdrop() -> Handle {
        Handle::from_rgba(1200, 675, vec![255; 1200 * 675 * 4])
    }

    #[test]
    fn artwork_residency_is_bounded_and_stale_uploads_cannot_replace_current_requests() {
        {
            let mut cache = BackdropCache::default();
            let handles: Vec<_> = (0..12).map(|_| backdrop()).collect();
            let mut duplicated = handles.clone();
            duplicated.extend(handles.clone());
            assert!(
                !cache.reconcile(duplicated).is_empty(),
                "some artwork is admitted"
            );
            assert!(
                cache.reconcile(handles).is_empty(),
                "duplicate and pending artwork is not reallocated and reservations are bounded"
            );
            assert!(
                cache.pending.values().map(|(_, size)| size).sum::<u64>() <= MAX_BACKDROP_BYTES,
                "duplicate and pending artwork is not reallocated and reservations are bounded"
            );
        }
        {
            let mut cache = BackdropCache::default();
            let handle = backdrop();
            let old = cache.reconcile(vec![handle.clone()])[0].0;
            cache.reconcile(vec![]);
            let new = cache.reconcile(vec![handle.clone()])[0].0;
            cache.complete(old, handle.id(), Err(Error::Unsupported));
            assert_eq!(cache.pending[&handle.id()].0, new);
            cache.complete(new, handle.id(), Err(Error::Unsupported));
            assert!(
                cache.reconcile(vec![handle.clone()]).is_empty(),
                "stale completions cannot remove a reopened request and failures do not retry forever"
            );
            cache.reconcile(vec![]);
            assert_eq!(cache.reconcile(vec![handle]).len(), 1);
        }
    }
}
