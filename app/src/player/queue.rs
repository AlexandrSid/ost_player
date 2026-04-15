use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub struct PlayerQueue {
    tracks: Vec<PathBuf>,
    order: Vec<usize>,
    pos_in_order: Option<usize>,
}

impl PlayerQueue {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn load(
        &mut self,
        tracks: Vec<PathBuf>,
        start_index: usize,
        shuffle: bool,
    ) -> Result<(), String> {
        if tracks.is_empty() {
            self.tracks = Vec::new();
            self.order = Vec::new();
            self.pos_in_order = None;
            return Ok(());
        }
        if start_index >= tracks.len() {
            return Err("start_index out of range".to_string());
        }
        self.tracks = tracks;
        self.rebuild_order(shuffle);
        self.pos_in_order = self
            .order
            .iter()
            .position(|&i| i == start_index)
            .or(Some(0));
        Ok(())
    }

    pub fn set_shuffle(&mut self, shuffle: bool) {
        let current_idx = self.current_index();
        self.rebuild_order(shuffle);
        self.pos_in_order = current_idx.and_then(|idx| self.order.iter().position(|&i| i == idx));
    }

    pub fn current_path(&self) -> Option<PathBuf> {
        let idx = self.current_index()?;
        self.tracks.get(idx).cloned()
    }

    pub fn path_at_pos_in_order(&self, pos_in_order: usize) -> Option<PathBuf> {
        let idx = *self.order.get(pos_in_order)?;
        self.tracks.get(idx).cloned()
    }

    pub fn current_index(&self) -> Option<usize> {
        let pos = self.pos_in_order?;
        self.order.get(pos).copied()
    }

    pub fn set_pos_in_order(&mut self, pos: usize) -> Result<(), String> {
        if pos >= self.order.len() {
            return Err("queue position out of range".to_string());
        }
        self.pos_in_order = Some(pos);
        Ok(())
    }

    pub fn pos_in_order(&self) -> Option<usize> {
        self.pos_in_order
    }

    pub fn order_len(&self) -> usize {
        self.order.len()
    }

    fn rebuild_order(&mut self, shuffle: bool) {
        self.order = (0..self.tracks.len()).collect();
        if shuffle {
            shuffle_in_place(&mut self.order);
        }
        if self.tracks.is_empty() {
            self.pos_in_order = None;
        } else if self.pos_in_order.is_none() {
            self.pos_in_order = Some(0);
        }
    }
}

fn shuffle_in_place(v: &mut [usize]) {
    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();
    v.shuffle(&mut rng);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    #[test]
    fn load_empty_clears_state() {
        let mut q = PlayerQueue::default();
        q.load(vec![], 0, false).unwrap();
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        assert_eq!(q.order_len(), 0);
        assert_eq!(q.pos_in_order(), None);
        assert_eq!(q.current_index(), None);
        assert_eq!(q.current_path(), None);
    }

    #[test]
    fn set_shuffle_on_empty_is_noop() {
        let mut q = PlayerQueue::default();
        q.set_shuffle(true);
        assert!(q.is_empty());
        assert_eq!(q.len(), 0);
        assert_eq!(q.order_len(), 0);
        assert_eq!(q.pos_in_order(), None);
        assert_eq!(q.current_index(), None);
        assert_eq!(q.current_path(), None);
    }

    #[test]
    fn load_start_index_sets_current_index_even_when_shuffled() {
        let tracks = vec![p("a"), p("b"), p("c"), p("d")];
        let mut q = PlayerQueue::default();
        q.load(tracks.clone(), 2, true).unwrap();
        assert_eq!(q.current_index(), Some(2));
        assert_eq!(q.current_path(), Some(p("c")));

        // Order must be a permutation of 0..len.
        let mut order = (0..q.order_len()).collect::<Vec<_>>();
        order.sort();
        assert_eq!(order, vec![0, 1, 2, 3]);
    }

    #[test]
    fn set_shuffle_preserves_current_track_when_possible() {
        let tracks = vec![p("a"), p("b"), p("c"), p("d"), p("e")];
        let mut q = PlayerQueue::default();
        q.load(tracks, 3, false).unwrap();
        let before_idx = q.current_index();
        let before_path = q.current_path();

        q.set_shuffle(true);
        assert_eq!(q.current_index(), before_idx);
        assert_eq!(q.current_path(), before_path);

        q.set_shuffle(false);
        assert_eq!(q.current_index(), before_idx);
        assert_eq!(q.current_path(), before_path);
    }

    #[test]
    fn load_rejects_out_of_range_start_index() {
        let tracks = vec![p("a"), p("b")];
        let mut q = PlayerQueue::default();
        assert!(q.load(tracks, 99, false).is_err());
    }

    #[test]
    fn set_pos_in_order_bounds_check() {
        let tracks = vec![p("a"), p("b"), p("c")];
        let mut q = PlayerQueue::default();
        q.load(tracks, 0, false).unwrap();
        assert!(q.set_pos_in_order(2).is_ok());
        assert!(q.set_pos_in_order(3).is_err());
    }
}
