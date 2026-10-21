//! The providers the plugins have, as `plugin providers` and the sidecar list them.

use locallibrary::plugin::Plugins;
use loclib_plugin::ProviderKind;
use schemars::JsonSchema;
use serde::Serialize;

/// The providers the plugins have, and when one's candidate is picked for the user, as `plugin
/// providers` and the sidecar's `plugins.providers` show them.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Providers {
    /// In the order the plugins are.
    pub providers: Vec<ListedProvider>,
    /// A candidate alone scoring this or more (0 to 1) is picked for the user: the
    /// `providers.auto_pick` setting.
    pub auto_pick: f64,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ListedProvider {
    pub plugin: String,
    pub name: String,
    pub title: String,
    pub kind: ProviderKind,
}

impl Providers {
    pub fn of(plugins: &Plugins, store: Option<&locallibrary::Store>) -> Providers {
        let providers = plugins.providers(None).into_iter().map(|(plugin, p)| ListedProvider {
            plugin: plugin.into(),
            name: p.name.clone(),
            title: p.title.clone(),
            kind: p.kind,
        });
        Providers { providers: providers.collect(), auto_pick: locallibrary::plugin::auto_pick(store) }
    }
}
