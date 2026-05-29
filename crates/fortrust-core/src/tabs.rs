use compact_str::CompactString;

use crate::config::PerformanceConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabStatus {
    Active { renderer_mb: u16 },
    Warm { renderer_mb: u16 },
    Suspended { snapshot_kb: u16 },
    Discarded,
}

impl TabStatus {
    pub fn estimated_mb(&self) -> u32 {
        match self {
            Self::Active { renderer_mb } | Self::Warm { renderer_mb } => u32::from(*renderer_mb),
            Self::Suspended { snapshot_kb } => u32::from(*snapshot_kb).div_ceil(1024).max(1),
            Self::Discarded => 0,
        }
    }

    pub fn is_background_renderer(&self) -> bool {
        matches!(self, Self::Warm { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub id: TabId,
    pub title: CompactString,
    pub url: CompactString,
    pub status: TabStatus,
    pub pinned: bool,
    pub private: bool,
    pub last_active_tick: u64,
    pub privacy_blocks: u32,
}

impl Tab {
    pub fn estimated_mb(&self) -> u32 {
        self.status.estimated_mb()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryReport {
    pub active_tabs: usize,
    pub warm_tabs: usize,
    pub suspended_tabs: usize,
    pub discarded_tabs: usize,
    pub total_estimated_mb: u32,
    pub budget_mb: u32,
}

#[derive(Debug, Clone)]
pub struct TabManager {
    tabs: Vec<Tab>,
    active: Option<TabId>,
    next_id: u64,
    tick: u64,
    settings: PerformanceConfig,
}

impl TabManager {
    pub fn new(settings: PerformanceConfig) -> Self {
        Self {
            tabs: Vec::new(),
            active: None,
            next_id: 1,
            tick: 0,
            settings,
        }
    }

    pub fn open_tab(
        &mut self,
        url: impl Into<CompactString>,
        title: impl Into<CompactString>,
        private: bool,
    ) -> TabId {
        self.tick += 1;
        self.demote_active_to_warm();

        let id = TabId(self.next_id);
        self.next_id += 1;
        self.active = Some(id);
        self.tabs.push(Tab {
            id,
            title: title.into(),
            url: url.into(),
            status: TabStatus::Active {
                renderer_mb: self.settings.max_active_renderer_mb,
            },
            pinned: false,
            private,
            last_active_tick: self.tick,
            privacy_blocks: 0,
        });
        self.enforce_memory_policy();
        id
    }

    pub fn activate(&mut self, id: TabId) -> bool {
        if self.active == Some(id) {
            return true;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };

        self.tick += 1;
        self.demote_active_to_warm();
        self.active = Some(id);

        let tab = &mut self.tabs[index];
        tab.status = TabStatus::Active {
            renderer_mb: self.settings.max_active_renderer_mb,
        };
        tab.last_active_tick = self.tick;
        self.enforce_memory_policy();
        true
    }

    pub fn close_tab(&mut self, id: TabId) -> bool {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };

        let was_active = self.active == Some(id);
        self.tabs.remove(index);
        if was_active {
            let next_active = self.tabs.last().map(|tab| tab.id);
            self.active = None;
            if let Some(active) = next_active {
                let _ = self.activate(active);
            }
        }
        true
    }

    pub fn navigate_active(
        &mut self,
        url: impl Into<CompactString>,
        title: impl Into<CompactString>,
    ) -> bool {
        let Some(active) = self.active else {
            return false;
        };

        self.navigate_tab(active, url, title)
    }

    pub fn navigate_tab(
        &mut self,
        id: TabId,
        url: impl Into<CompactString>,
        title: impl Into<CompactString>,
    ) -> bool {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            return false;
        };

        tab.url = url.into();
        tab.title = title.into();
        true
    }

    pub fn record_privacy_block(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.privacy_blocks = tab.privacy_blocks.saturating_add(1);
        }
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    pub fn active_id(&self) -> Option<TabId> {
        self.active
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        let active = self.active?;
        self.tabs.iter().find(|tab| tab.id == active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        let active = self.active?;
        self.tabs.iter_mut().find(|tab| tab.id == active)
    }

    pub fn memory_report(&self) -> MemoryReport {
        let mut report = MemoryReport {
            active_tabs: 0,
            warm_tabs: 0,
            suspended_tabs: 0,
            discarded_tabs: 0,
            total_estimated_mb: 0,
            budget_mb: self.settings.max_total_tab_ram_mb,
        };

        for tab in &self.tabs {
            report.total_estimated_mb += tab.estimated_mb();
            match tab.status {
                TabStatus::Active { .. } => report.active_tabs += 1,
                TabStatus::Warm { .. } => report.warm_tabs += 1,
                TabStatus::Suspended { .. } => report.suspended_tabs += 1,
                TabStatus::Discarded => report.discarded_tabs += 1,
            }
        }

        report
    }

    pub fn reorder_tab(&mut self, id: TabId, new_index: usize) -> bool {
        let Some(old_index) = self.tabs.iter().position(|tab| tab.id == id) else {
            return false;
        };
        let new_index = new_index.min(self.tabs.len() - 1);
        if old_index == new_index { return true; }
        let tab = self.tabs.remove(old_index);
        self.tabs.insert(new_index, tab);
        true
    }

    pub fn enforce_memory_policy(&mut self) {
        self.suspend_stale_background_tabs();
        self.limit_warm_tabs();
        self.fit_total_budget();
    }

    fn demote_active_to_warm(&mut self) {
        let Some(active) = self.active else {
            return;
        };

        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == active) {
            tab.status = TabStatus::Warm {
                renderer_mb: self.settings.max_warm_renderer_mb,
            };
        }
    }

    fn suspend_stale_background_tabs(&mut self) {
        let active = self.active;
        for tab in &mut self.tabs {
            if Some(tab.id) == active || tab.pinned {
                continue;
            }
            if self.tick.saturating_sub(tab.last_active_tick)
                >= self.settings.suspend_background_after_ticks
            {
                tab.status = TabStatus::Suspended {
                    snapshot_kb: self.settings.suspended_snapshot_kb,
                };
            }
        }
    }

    fn limit_warm_tabs(&mut self) {
        let active = self.active;
        let mut warm = self
            .tabs
            .iter()
            .filter(|tab| Some(tab.id) != active && tab.status.is_background_renderer())
            .map(|tab| (tab.id, tab.last_active_tick))
            .collect::<Vec<_>>();

        warm.sort_by_key(|(_, last_active_tick)| *last_active_tick);
        let overflow = warm.len().saturating_sub(self.settings.warm_tab_limit);
        for (id, _) in warm.into_iter().take(overflow) {
            self.suspend_tab(id);
        }
    }

    fn fit_total_budget(&mut self) {
        while self.memory_report().total_estimated_mb > self.settings.max_total_tab_ram_mb {
            let Some(id) = self
                .tabs
                .iter()
                .filter(|tab| {
                    Some(tab.id) != self.active
                        && !tab.pinned
                        && !matches!(
                            tab.status,
                            TabStatus::Suspended { .. } | TabStatus::Discarded
                        )
                })
                .max_by_key(|tab| tab.estimated_mb())
                .map(|tab| tab.id)
            else {
                break;
            };
            self.suspend_tab(id);
        }

        while self.memory_report().total_estimated_mb > self.settings.max_total_tab_ram_mb {
            let Some(id) = self
                .tabs
                .iter()
                .filter(|tab| {
                    Some(tab.id) != self.active
                        && !tab.pinned
                        && matches!(tab.status, TabStatus::Suspended { .. })
                })
                .min_by_key(|tab| tab.last_active_tick)
                .map(|tab| tab.id)
            else {
                break;
            };
            self.discard_tab(id);
        }
    }

    fn suspend_tab(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.status = TabStatus::Suspended {
                snapshot_kb: self.settings.suspended_snapshot_kb,
            };
        }
    }

    fn discard_tab(&mut self, id: TabId) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.status = TabStatus::Discarded;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn many_tabs_remain_inside_ram_budget() {
        let mut manager = TabManager::new(PerformanceConfig::default());

        for index in 0..50 {
            manager.open_tab(
                format!("https://example{index}.com"),
                format!("Tab {index}"),
                false,
            );
        }

        let report = manager.memory_report();
        assert_eq!(report.active_tabs, 1);
        assert!(report.warm_tabs <= 2);
        assert!(report.suspended_tabs >= 47);
        assert!(report.total_estimated_mb <= report.budget_mb);
    }

    #[test]
    fn activating_suspended_tab_restores_only_that_tab() {
        let mut manager = TabManager::new(PerformanceConfig::default());
        let first = manager.open_tab("https://first.test", "First", false);

        for index in 0..8 {
            manager.open_tab(
                format!("https://other{index}.test"),
                format!("Other {index}"),
                false,
            );
        }

        assert!(manager.activate(first));
        let first_tab = manager.active_tab().expect("active tab should exist");
        assert_eq!(first_tab.id, first);
        assert!(matches!(first_tab.status, TabStatus::Active { .. }));
        assert!(manager.memory_report().total_estimated_mb <= manager.memory_report().budget_mb);
    }

    #[test]
    fn massive_tab_sets_discard_cold_tabs_to_stay_inside_budget() {
        let mut manager = TabManager::new(PerformanceConfig::default());

        for index in 0..1_000 {
            manager.open_tab(
                format!("https://bulk{index}.test"),
                format!("Bulk {index}"),
                false,
            );
        }

        let report = manager.memory_report();
        assert_eq!(report.active_tabs, 1);
        assert!(report.warm_tabs <= 2);
        assert!(report.discarded_tabs > 0);
        assert!(report.total_estimated_mb <= report.budget_mb);
    }
}
