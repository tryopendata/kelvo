//! The size tree a scan builds. Every folder's total is exact; what is kept below a
//! finished folder follows the display rule (its 120 largest entries that are at least
//! 0.5% of it), and everything else folds into that folder's "smaller items".
use std::collections::HashMap;

use kelvo_schema::disk_space::{SpaceKind, SpaceNode};

/// Entries a folder keeps after it finishes, and that a breakdown shows.
pub const MAX_CHILDREN: usize = 120;
/// An entry below this share of its folder (1/200 = 0.5%) is merged.
pub const MIN_SHARE_DIVISOR: u64 = 200;
/// Live nodes above which files stop being kept; totals stay exact.
pub const NODE_CAP: usize = 400_000;

pub type NodeId = u32;

#[derive(Debug)]
pub(crate) struct Node {
    pub parent: Option<NodeId>,
    pub name: Box<str>,
    pub bytes: u64,
    pub items: u64,
    pub dir: bool,
    pub done: bool,
    /// Subfolders not yet finished (folders only).
    pub pending: u32,
    pub children: Vec<NodeId>,
    pub smaller_bytes: u64,
    pub smaller_items: u64,
    pub smaller_count: u64,
}

impl Node {
    fn new(parent: Option<NodeId>, name: Box<str>, dir: bool) -> Self {
        Self {
            parent,
            name,
            bytes: 0,
            items: 0,
            dir,
            done: !dir,
            pending: 0,
            children: Vec::new(),
            smaller_bytes: 0,
            smaller_items: 0,
            smaller_count: 0,
        }
    }

    pub fn view(&self, id: NodeId) -> SpaceNode {
        SpaceNode {
            id: Some(id),
            name: self.name.to_string(),
            kind: if self.dir {
                SpaceKind::Folder
            } else {
                SpaceKind::File
            },
            bytes: self.bytes as f64,
            items: self.items as f64,
            done: self.done,
            openable: self.dir && self.bytes > 0,
        }
    }
}

/// One file seen in a listing.
pub(crate) struct FileEntry {
    pub name: Box<str>,
    pub bytes: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Tree {
    nodes: HashMap<NodeId, Node>,
    next: NodeId,
    pub truncated: bool,
    /// Entries smaller than this are merged wherever they are, so a deep tree of small
    /// folders (`node_modules`) does not keep every level. Totals are unaffected.
    pub min_bytes: u64,
}

impl Tree {
    pub fn with_root(name: &str) -> (Self, NodeId) {
        let mut tree = Self::default();
        let root = tree.insert(Node::new(None, name.into(), true));
        (tree, root)
    }

    fn insert(&mut self, node: Node) -> NodeId {
        let id = self.next;
        self.next = self.next.wrapping_add(1);
        self.nodes.insert(id, node);
        id
    }

    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Records one folder's listing: its subfolders become pending children, its largest
    /// files become file nodes and the rest fold into its smaller items. Returns the new
    /// subfolder ids in `dirs` order. A folder with no subfolders finishes here.
    pub fn record_listing(
        &mut self,
        id: NodeId,
        dirs: Vec<Box<str>>,
        mut files: Vec<FileEntry>,
    ) -> Vec<NodeId> {
        let keep_files = !self.truncated;
        files.sort_unstable_by_key(|f| std::cmp::Reverse(f.bytes));
        let (bytes, items) = (
            files.iter().map(|f| f.bytes).sum::<u64>(),
            files.len() as u64,
        );
        let mut children = Vec::with_capacity(dirs.len() + files.len().min(MAX_CHILDREN));
        let mut folded = (0, 0);
        for (i, file) in files.into_iter().enumerate() {
            if keep_files && i < MAX_CHILDREN && file.bytes > 0 && file.bytes >= self.min_bytes {
                let mut node = Node::new(Some(id), file.name, false);
                node.bytes = file.bytes;
                node.items = 1;
                children.push(self.insert(node));
            } else if file.bytes > 0 {
                folded = (folded.0 + file.bytes, folded.1 + 1);
            }
        }
        let subdirs: Vec<NodeId> = dirs
            .into_iter()
            .map(|name| self.insert(Node::new(Some(id), name, true)))
            .collect();
        children.extend(&subdirs);
        if self.nodes.len() > NODE_CAP {
            self.truncated = true;
        }
        let Some(node) = self.nodes.get_mut(&id) else {
            return Vec::new();
        };
        node.children.extend(children);
        node.smaller_bytes += folded.0;
        node.smaller_items += folded.1;
        node.smaller_count += folded.1;
        node.pending = subdirs.len() as u32;
        self.add_up(id, bytes, items);
        if subdirs.is_empty() {
            self.finish(id);
        }
        subdirs
    }

    /// Adds bytes and items to a folder and every ancestor.
    fn add_up(&mut self, mut id: NodeId, bytes: u64, items: u64) {
        while let Some(node) = self.nodes.get_mut(&id) {
            node.bytes += bytes;
            node.items += items;
            match node.parent {
                Some(parent) => id = parent,
                None => break,
            }
        }
    }

    /// Finishes a folder that could not be listed (unreadable, another device).
    pub fn finish_empty(&mut self, id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&id) {
            node.pending = 0;
        }
        self.finish(id);
    }

    /// Marks a folder done, prunes it to the display rule, and finishes each ancestor whose
    /// last pending subfolder this was. Returns the ids that finished, innermost first.
    pub fn finish(&mut self, id: NodeId) -> Vec<NodeId> {
        let mut finished = Vec::new();
        let mut next = Some(id);
        while let Some(id) = next {
            self.prune(id);
            finished.push(id);
            next = None;
            let parent = self.nodes.get(&id).and_then(|n| n.parent);
            if let Some(parent) = parent.and_then(|p| self.nodes.get_mut(&p).map(|n| (p, n))) {
                parent.1.pending = parent.1.pending.saturating_sub(1);
                if parent.1.pending == 0 {
                    next = Some(parent.0);
                }
            }
        }
        finished
    }

    fn prune(&mut self, id: NodeId) {
        let Some(node) = self.nodes.get_mut(&id) else {
            return;
        };
        node.done = true;
        let total = node.bytes;
        let mut kids = std::mem::take(&mut node.children);
        kids.sort_unstable_by_key(|k| std::cmp::Reverse(self.nodes.get(k).map_or(0, |n| n.bytes)));
        let mut kept = Vec::with_capacity(kids.len().min(MAX_CHILDREN));
        let mut folded = (0, 0, 0);
        for (i, kid) in kids.into_iter().enumerate() {
            let (bytes, items) = self.nodes.get(&kid).map_or((0, 0), |n| (n.bytes, n.items));
            let big = bytes > 0
                && bytes >= self.min_bytes
                && bytes.saturating_mul(MIN_SHARE_DIVISOR) >= total;
            if i < MAX_CHILDREN && big {
                kept.push(kid);
            } else {
                if bytes > 0 {
                    folded = (folded.0 + bytes, folded.1 + items, folded.2 + 1);
                }
                self.remove_subtree(kid);
            }
        }
        if let Some(node) = self.nodes.get_mut(&id) {
            node.children = kept;
            node.smaller_bytes += folded.0;
            node.smaller_items += folded.1;
            node.smaller_count += folded.2;
        }
    }

    fn remove_subtree(&mut self, id: NodeId) {
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            if let Some(node) = self.nodes.remove(&id) {
                stack.extend(node.children);
            }
        }
    }

    /// The names from the root to `id`, root excluded.
    pub fn rel_path(&self, id: NodeId) -> Option<Vec<&str>> {
        let mut out = Vec::new();
        let mut cur = self.nodes.get(&id)?;
        while let Some(parent) = cur.parent {
            out.push(&*cur.name);
            cur = self.nodes.get(&parent)?;
        }
        out.reverse();
        Some(out)
    }

    /// `(id, name)` from the root to `id`, inclusive.
    pub fn crumbs(&self, id: NodeId) -> Vec<(NodeId, String)> {
        let mut out = Vec::new();
        let mut next = Some(id);
        while let Some(id) = next {
            let Some(node) = self.nodes.get(&id) else {
                break;
            };
            out.push((id, node.name.to_string()));
            next = node.parent;
        }
        out.reverse();
        out
    }

    /// A folder's entries largest first, at most `MAX_CHILDREN` of them, the rest (and the
    /// folder's already-merged entries) as one smaller-items entry.
    pub fn breakdown(&self, id: NodeId) -> Vec<SpaceNode> {
        let Some(node) = self.nodes.get(&id) else {
            return Vec::new();
        };
        let mut kids: Vec<SpaceNode> = node
            .children
            .iter()
            .filter_map(|k| self.nodes.get(k).map(|n| n.view(*k)))
            .collect();
        kids.sort_by(|a, b| {
            b.bytes
                .total_cmp(&a.bytes)
                .then_with(|| a.name.cmp(&b.name))
        });
        let (mut bytes, mut items, mut count) = (
            node.smaller_bytes as f64,
            node.smaller_items as f64,
            node.smaller_count as f64,
        );
        if kids.len() > MAX_CHILDREN {
            for extra in kids.drain(MAX_CHILDREN..) {
                bytes += extra.bytes;
                items += extra.items;
                count += 1.0;
            }
        }
        if bytes > 0.0 {
            kids.push(SpaceNode {
                id: None,
                name: match count as u64 {
                    1 => "1 smaller item".to_owned(),
                    n => format!("{n} smaller items"),
                },
                kind: SpaceKind::Smaller,
                bytes,
                items,
                done: node.done,
                openable: false,
            });
        }
        kids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(sizes: &[u64]) -> Vec<FileEntry> {
        sizes
            .iter()
            .enumerate()
            .map(|(i, b)| FileEntry {
                name: format!("f{i}").into(),
                bytes: *b,
            })
            .collect()
    }

    #[test]
    fn totals_propagate_and_folders_finish_bottom_up() {
        let (mut t, root) = Tree::with_root("root");
        let subs = t.record_listing(root, vec!["a".into(), "b".into()], files(&[10]));
        assert!(!t.get(root).unwrap().done);
        t.record_listing(subs[0], vec![], files(&[100, 50]));
        assert!(t.get(subs[0]).unwrap().done);
        assert!(!t.get(root).unwrap().done, "b is still pending");
        assert_eq!(t.get(root).unwrap().bytes, 160);
        t.record_listing(subs[1], vec![], files(&[1000]));
        let r = t.get(root).unwrap();
        assert!(r.done);
        assert_eq!((r.bytes, r.items), (1160, 4));
    }

    #[test]
    fn finishing_merges_entries_under_half_a_percent() {
        let (mut t, root) = Tree::with_root("root");
        t.record_listing(root, vec![], files(&[10_000, 49, 51, 0]));
        let view = t.breakdown(root);
        let names: Vec<_> = view
            .iter()
            .map(|n| (n.name.as_str(), n.bytes as u64))
            .collect();
        // 51 is at least 0.5% of 10,100; 49 is under it; zero-byte files are dropped.
        assert_eq!(names, [("f0", 10_000), ("f2", 51), ("1 smaller item", 49)]);
        assert_eq!(view[2].kind, SpaceKind::Smaller);
    }

    #[test]
    fn a_folder_keeps_at_most_the_display_limit_of_entries() {
        let (mut t, root) = Tree::with_root("root");
        t.record_listing(root, vec![], files(&[100; 150]));
        let view = t.breakdown(root);
        assert_eq!(view.len(), MAX_CHILDREN + 1);
        assert_eq!(view[MAX_CHILDREN].bytes as u64, 30 * 100);
        assert_eq!(t.get(root).unwrap().bytes, 15_000, "totals stay exact");
    }

    #[test]
    fn pruned_folders_free_their_subtrees() {
        let (mut t, root) = Tree::with_root("root");
        let subs = t.record_listing(root, vec!["big".into(), "tiny".into()], vec![]);
        t.record_listing(subs[0], vec![], files(&[1_000_000]));
        let deep = t.record_listing(subs[1], vec!["x".into()], vec![]);
        t.record_listing(deep[0], vec![], files(&[1, 1, 1]));
        assert!(t.get(root).unwrap().done);
        assert!(t.get(subs[1]).is_none(), "tiny folded into smaller items");
        assert!(t.get(deep[0]).is_none());
        assert_eq!(t.len(), 3, "root, big, and big's file");
    }

    #[test]
    fn unreadable_folders_finish_without_bytes() {
        let (mut t, root) = Tree::with_root("root");
        let subs = t.record_listing(root, vec!["locked".into()], files(&[5]));
        t.finish_empty(subs[0]);
        assert!(t.get(root).unwrap().done);
        assert_eq!(t.get(root).unwrap().bytes, 5);
    }

    #[test]
    fn a_running_folder_lists_pending_children_with_partial_sizes() {
        let (mut t, root) = Tree::with_root("root");
        let subs = t.record_listing(root, vec!["a".into(), "b".into()], vec![]);
        let deep = t.record_listing(subs[0], vec!["c".into()], files(&[70]));
        let view = t.breakdown(root);
        assert_eq!(view[0].name, "a");
        assert!(!view[0].done && view[0].bytes as u64 == 70);
        assert!(view[0].openable);
        assert_eq!(t.rel_path(deep[0]).unwrap(), ["a", "c"]);
        assert_eq!(t.crumbs(deep[0]).len(), 3);
    }
}
