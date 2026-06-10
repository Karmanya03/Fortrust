use compact_str::CompactString;
use fortrust_privacy::fingerprint::FingerprintGuard;

use crate::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub u64);

/// Per-workspace container identity configuration.
/// Each workspace can have its own fingerprint profile, sandbox config,
/// and storage partition key for complete identity isolation.
#[derive(Debug, Clone, PartialEq)]
pub struct ContainerConfig {
    /// Unique fingerprint noise seed for this workspace.
    pub fingerprint_seed: u64,
    /// Whether to use container-level sandboxing (restricted APIs, isolated storage).
    pub sandboxed: bool,
}

impl Default for ContainerConfig {
    fn default() -> Self {
        Self {
            fingerprint_seed: rand::random::<u64>(),
            sandboxed: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: CompactString,
    pub color_hex: CompactString,
    pub tab_ids: Vec<TabId>,
    /// Per-workspace fingerprint guard for canvas noise, UA override, etc.
    pub fingerprint_guard: FingerprintGuard,
    /// Container-level configuration for this workspace.
    pub container_config: ContainerConfig,
    /// Isolated storage partition key (e.g., for separate cookie jar).
    /// `None` means use the default (global) storage partition.
    pub storage_partition_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceManager {
    workspaces: Vec<Workspace>,
    active: WorkspaceId,
    next_id: u64,
}

impl WorkspaceManager {
    pub fn new() -> Self {
        let default_id = WorkspaceId(1);
        let default_seed = rand::random::<u64>();
        Self {
            workspaces: vec![Workspace {
                id: default_id,
                name: CompactString::from("Default"),
                color_hex: CompactString::from("#4d9fff"),
                tab_ids: Vec::new(),
                fingerprint_guard: FingerprintGuard::with_seed(default_seed),
                container_config: ContainerConfig {
                    fingerprint_seed: default_seed,
                    sandboxed: false,
                },
                storage_partition_key: None,
            }],
            active: default_id,
            next_id: 2,
        }
    }

    pub fn create(&mut self, name: impl Into<CompactString>, color_hex: impl Into<CompactString>) -> WorkspaceId {
        let id = WorkspaceId(self.next_id);
        self.next_id += 1;
        let seed = rand::random::<u64>();
        self.workspaces.push(Workspace {
            id,
            name: name.into(),
            color_hex: color_hex.into(),
            tab_ids: Vec::new(),
            fingerprint_guard: FingerprintGuard::with_seed(seed),
            container_config: ContainerConfig {
                fingerprint_seed: seed,
                sandboxed: false,
            },
            storage_partition_key: None,
        });
        id
    }

    /// Create a workspace as a privacy container with isolated identity.
    /// Automatically generates a unique fingerprint seed for this container.
    pub fn create_container(
        &mut self,
        name: impl Into<CompactString>,
        color_hex: impl Into<CompactString>,
    ) -> WorkspaceId {
        let id = WorkspaceId(self.next_id);
        self.next_id += 1;
        let seed = rand::random::<u64>();
        let partition_key = format!("container-{:016x}", id.0);
        self.workspaces.push(Workspace {
            id,
            name: name.into(),
            color_hex: color_hex.into(),
            tab_ids: Vec::new(),
            fingerprint_guard: FingerprintGuard::with_seed(seed),
            container_config: ContainerConfig {
                fingerprint_seed: seed,
                sandboxed: true,
            },
            storage_partition_key: Some(partition_key),
        });
        id
    }

    pub fn rename(&mut self, id: WorkspaceId, name: impl Into<CompactString>) -> bool {
        if let Some(ws) = self.workspaces.iter_mut().find(|ws| ws.id == id) {
            ws.name = name.into();
            true
        } else {
            false
        }
    }

    pub fn set_color(&mut self, id: WorkspaceId, color_hex: impl Into<CompactString>) -> bool {
        if let Some(ws) = self.workspaces.iter_mut().find(|ws| ws.id == id) {
            ws.color_hex = color_hex.into();
            true
        } else {
            false
        }
    }

    pub fn delete(&mut self, id: WorkspaceId) -> bool {
        if id == WorkspaceId(1) {
            return false;
        }
        let Some(pos) = self.workspaces.iter().position(|ws| ws.id == id) else {
            return false;
        };
        let removed = self.workspaces.remove(pos);
        // Move orphaned tabs to default workspace
        let default_id = WorkspaceId(1);
        if let Some(default) = self.workspaces.iter_mut().find(|ws| ws.id == default_id) {
            default.tab_ids.extend(removed.tab_ids);
        }
        if self.active == id {
            self.active = default_id;
        }
        true
    }

    pub fn activate(&mut self, id: WorkspaceId) -> bool {
        if self.workspaces.iter().any(|ws| ws.id == id) {
            self.active = id;
            true
        } else {
            false
        }
    }

    pub fn add_tab(&mut self, workspace_id: WorkspaceId, tab_id: TabId) -> bool {
        if let Some(ws) = self.workspaces.iter_mut().find(|ws| ws.id == workspace_id) {
            if !ws.tab_ids.contains(&tab_id) {
                ws.tab_ids.push(tab_id);
            }
            true
        } else {
            false
        }
    }

    pub fn remove_tab(&mut self, tab_id: TabId) {
        for ws in &mut self.workspaces {
            ws.tab_ids.retain(|&id| id != tab_id);
        }
    }

    pub fn move_tab(&mut self, tab_id: TabId, target_workspace: WorkspaceId) -> bool {
        self.remove_tab(tab_id);
        self.add_tab(target_workspace, tab_id)
    }

    pub fn active(&self) -> WorkspaceId {
        self.active
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id == self.active)
    }

    pub fn active_workspace_mut(&mut self) -> Option<&mut Workspace> {
        self.workspaces.iter_mut().find(|ws| ws.id == self.active)
    }

    pub fn get(&self, id: WorkspaceId) -> Option<&Workspace> {
        self.workspaces.iter().find(|ws| ws.id == id)
    }

    pub fn all(&self) -> &[Workspace] {
        &self.workspaces
    }

    pub fn workspace_for_tab(&self, tab_id: TabId) -> Option<WorkspaceId> {
        self.workspaces
            .iter()
            .find(|ws| ws.tab_ids.contains(&tab_id))
            .map(|ws| ws.id)
    }

    // ── Container identity accessors ──────────────────────────────────

    /// Get the fingerprint guard for a given workspace.
    /// Returns `None` if the workspace does not exist.
    pub fn fingerprint_guard(&self, id: WorkspaceId) -> Option<&FingerprintGuard> {
        self.workspaces
            .iter()
            .find(|ws| ws.id == id)
            .map(|ws| &ws.fingerprint_guard)
    }

    /// Get a mutable reference to the fingerprint guard for a workspace.
    pub fn fingerprint_guard_mut(&mut self, id: WorkspaceId) -> Option<&mut FingerprintGuard> {
        self.workspaces
            .iter_mut()
            .find(|ws| ws.id == id)
            .map(|ws| &mut ws.fingerprint_guard)
    }

    /// Regenerate the fingerprint seed for a workspace.
    /// This creates a new identity profile for all future requests/tabs in this container.
    pub fn regenerate_fingerprint(&mut self, id: WorkspaceId) -> bool {
        if let Some(ws) = self.workspaces.iter_mut().find(|ws| ws.id == id) {
            ws.container_config.fingerprint_seed = rand::random::<u64>();
            ws.fingerprint_guard.regenerate_seed();
            true
        } else {
            false
        }
    }

    /// Get the container config for a workspace.
    pub fn container_config(&self, id: WorkspaceId) -> Option<&ContainerConfig> {
        self.workspaces
            .iter()
            .find(|ws| ws.id == id)
            .map(|ws| &ws.container_config)
    }

    /// Get the storage partition key for a workspace.
    /// Returns `None` for the default workspace (shared storage).
    pub fn storage_partition_key(&self, id: WorkspaceId) -> Option<&str> {
        self.workspaces
            .iter()
            .find(|ws| ws.id == id)?
            .storage_partition_key
            .as_deref()
    }

    /// Check whether a workspace is a sandboxed container.
    pub fn is_container(&self, id: WorkspaceId) -> bool {
        self.workspaces
            .iter()
            .find(|ws| ws.id == id)
            .is_some_and(|ws| ws.container_config.sandboxed)
    }
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_workspace_has_fingerprint_guard() {
        let manager = WorkspaceManager::new();
        let guard = manager.fingerprint_guard(WorkspaceId(1)).expect("default workspace has guard");
        assert_eq!(guard.hardware_concurrency, 4);
        assert_eq!(guard.device_memory, 8.0);
        assert!(guard.canvas.enabled);
        assert!(guard.audio.enabled);
    }

    #[test]
    fn default_workspace_has_no_storage_partition() {
        let manager = WorkspaceManager::new();
        assert_eq!(manager.storage_partition_key(WorkspaceId(1)), None);
    }

    #[test]
    fn default_workspace_is_not_container() {
        let manager = WorkspaceManager::new();
        assert!(!manager.is_container(WorkspaceId(1)));
    }

    #[test]
    fn create_workspace_gets_unique_fingerprint_seed() {
        let mut manager = WorkspaceManager::new();
        let id_a = manager.create("Work", "#ff0000");
        let id_b = manager.create("Personal", "#00ff00");

        let seed_a = manager.container_config(id_a).unwrap().fingerprint_seed;
        let seed_b = manager.container_config(id_b).unwrap().fingerprint_seed;
        assert_ne!(seed_a, seed_b, "each workspace must have a unique fingerprint seed");
    }

    #[test]
    fn create_container_has_sandboxed_true_and_partition_key() {
        let mut manager = WorkspaceManager::new();
        let id = manager.create_container("Shopping", "#ff6600");

        assert!(manager.is_container(id));
        assert!(manager.container_config(id).unwrap().sandboxed);

        let partition = manager.storage_partition_key(id);
        assert!(partition.is_some(), "container must have a storage partition key");
        assert!(partition.unwrap().starts_with("container-"));
    }

    #[test]
    fn create_container_has_isolated_fingerprint_guard() {
        let mut manager = WorkspaceManager::new();
        let id = manager.create_container("Banking", "#0044ff");

        let guard = manager.fingerprint_guard(id).unwrap();
        assert!(guard.canvas.enabled);
        assert!(guard.audio.enabled);
        // Verify it's using the container seed, not the default
        let container_seed = manager.container_config(id).unwrap().fingerprint_seed;
        let default_seed = manager.container_config(WorkspaceId(1)).unwrap().fingerprint_seed;
        assert_ne!(container_seed, default_seed);
    }

    #[test]
    fn regenerate_fingerprint_changes_seed() {
        let mut manager = WorkspaceManager::new();
        let id = manager.create("Test", "#fff");

        let old_seed = manager.container_config(id).unwrap().fingerprint_seed;
        assert!(manager.regenerate_fingerprint(id));
        let new_seed = manager.container_config(id).unwrap().fingerprint_seed;
        assert_ne!(old_seed, new_seed);
    }

    #[test]
    fn regenerate_fingerprint_returns_false_for_nonexistent() {
        let mut manager = WorkspaceManager::new();
        assert!(!manager.regenerate_fingerprint(WorkspaceId(999)));
    }

    #[test]
    fn fingerprint_guard_mut_allows_modification() {
        let mut manager = WorkspaceManager::new();
        let id = manager.create("Test", "#fff");

        let guard = manager.fingerprint_guard_mut(id).unwrap();
        guard.hardware_concurrency = 8;
        guard.device_memory = 16.0;

        let guard = manager.fingerprint_guard(id).unwrap();
        assert_eq!(guard.hardware_concurrency, 8);
        assert_eq!(guard.device_memory, 16.0);
    }

    #[test]
    fn containers_have_unique_partition_keys() {
        let mut manager = WorkspaceManager::new();
        let id_a = manager.create_container("A", "#a00");
        let id_b = manager.create_container("B", "#0a0");

        let key_a = manager.storage_partition_key(id_a).unwrap().to_owned();
        let key_b = manager.storage_partition_key(id_b).unwrap().to_owned();
        assert_ne!(key_a, key_b, "container partition keys must be unique");
    }

    #[test]
    fn fingerprint_seeds_differ_across_containers() {
        let mut manager = WorkspaceManager::new();
        let id_a = manager.create_container("Container A", "#a00");
        let id_b = manager.create_container("Container B", "#0a0");
        let id_c = manager.create("Regular", "#00a");

        let seeds: Vec<u64> = [id_a, id_b, id_c]
            .iter()
            .map(|&id| manager.container_config(id).unwrap().fingerprint_seed)
            .collect();

        // All three should have different seeds
        assert_ne!(seeds[0], seeds[1]);
        assert_ne!(seeds[0], seeds[2]);
        assert_ne!(seeds[1], seeds[2]);
    }

    #[test]
    fn nonexistent_workspace_returns_none_for_config() {
        let mut manager = WorkspaceManager::new();
        assert!(manager.fingerprint_guard(WorkspaceId(999)).is_none());
        assert!(manager.fingerprint_guard_mut(WorkspaceId(999)).is_none());
        assert!(manager.container_config(WorkspaceId(999)).is_none());
        assert!(manager.storage_partition_key(WorkspaceId(999)).is_none());
        assert!(!manager.is_container(WorkspaceId(999)));
    }
}
