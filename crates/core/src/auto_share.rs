//! One per-project DAW save preference shared by Desktop and plugin adapters.
use crate::Engine;
use anyhow::Result;
use serde::Serialize;
use uuid::Uuid;

#[derive(Serialize)]
pub struct DawPluginSetting {
    pub project_id: Uuid,
    pub name: String,
    pub auto_share: bool,
}

impl Engine {
    /// Memory-only read; callers already bind/validate the linked project.
    pub fn auto_share_enabled(&self, project: Uuid) -> Result<bool> {
        Ok(self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .value
            .auto_share_projects
            .contains(&project))
    }
    pub fn set_auto_share(&self, project: Uuid, enabled: bool) -> Result<()> {
        let linked = self
            .store
            .projects()?
            .into_iter()
            .map(|item| item.project_id)
            .collect();
        self.settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?
            .set_auto_share(&self.directory, project, enabled, &linked)
    }
    /// Blocking adapter query; drops preference entries for projects no longer linked.
    pub fn daw_plugin_settings(&self) -> Result<Vec<DawPluginSetting>> {
        let projects = self.store.projects()?;
        let linked = projects.iter().map(|item| item.project_id).collect();
        let mut settings = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("settings lock poisoned"))?;
        settings.retain_auto_share_projects(&self.directory, &linked)?;
        Ok(projects
            .into_iter()
            .map(|project| DawPluginSetting {
                auto_share: settings
                    .value
                    .auto_share_projects
                    .contains(&project.project_id),
                project_id: project.project_id,
                name: project.name,
            })
            .collect())
    }
}
