use compact_str::CompactString;

use crate::TabId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorkspaceId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: CompactString,
    pub color_hex: CompactString,
    pub tab_ids: Vec<TabId>,
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
        Self {
            workspaces: vec![Workspace {
                id: default_id,
                name: CompactString::from("Default"),
                color_hex: CompactString::from("#4d9fff"),
                tab_ids: Vec::new(),
            }],
            active: default_id,
            next_id: 2,
        }
    }

    pub fn create(&mut self, name: impl Into<CompactString>, color_hex: impl Into<CompactString>) -> WorkspaceId {
        let id = WorkspaceId(self.next_id);
        self.next_id += 1;
        self.workspaces.push(Workspace {
            id,
            name: name.into(),
            color_hex: color_hex.into(),
            tab_ids: Vec::new(),
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
}

impl Default for WorkspaceManager {
    fn default() -> Self {
        Self::new()
    }
}
