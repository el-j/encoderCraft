//! Dependency layering rules (docs/architecture.md §2).
//!
//! Enforces that lower layers never depend on higher layers.

use serde_json::Value;

/// Where a workspace crate sits in the layering model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// Regular layered crate.
    Layer(u8),
    /// Standalone L0 crate: may not depend on any workspace crate.
    Standalone,
    /// Test tooling: may depend on anything up to L5; other crates may use only in dev-dependencies.
    Testkit,
    /// Binaries and build tooling: exempt from the rules.
    Exempt,
}

impl Class {
    fn layer(self) -> Option<u8> {
        match self {
            Class::Layer(l) => Some(l),
            Class::Standalone => Some(0),
            Class::Testkit => Some(6),
            Class::Exempt => None,
        }
    }
}

/// The layering table for encodercraft.
/// Names are package names without the `encodercraft-` prefix.
pub const TABLE: &[(&str, Class)] = &[
    ("geom", Class::Standalone),
    ("color", Class::Layer(0)),
    ("core", Class::Layer(1)),
    ("demux", Class::Layer(2)),
    ("video-codecs", Class::Layer(3)),
    ("audio-dsp", Class::Layer(3)),
    ("image-codecs", Class::Layer(3)),
    ("spatial-io", Class::Layer(3)),
    ("muxers", Class::Layer(3)),
    ("platform", Class::Layer(3)),
    ("pipeline", Class::Layer(4)),
    ("presets", Class::Layer(4)),
    ("watch", Class::Layer(4)),
    ("ipc", Class::Layer(4)),
    ("engine", Class::Layer(5)),
    ("ui-egui", Class::Layer(6)),
    ("mcp", Class::Layer(6)),
    ("testkit", Class::Testkit),
    ("dispatch", Class::Layer(1)),
    // Binaries and tooling (Exempt)
    ("encodercraft", Class::Exempt),
    ("cli", Class::Exempt),
    ("daemon", Class::Exempt),
    ("xtask", Class::Exempt),
];

pub fn check(metadata_json: &str) -> Result<(), String> {
    let meta: Value = serde_json::from_str(metadata_json).map_err(|e| format!("failed to parse cargo metadata JSON: {e}"))?;

    let packages = meta.get("packages").and_then(Value::as_array).ok_or("no packages array in cargo metadata")?;

    let workspace_members = meta.get("workspace_members").and_then(Value::as_array).ok_or("no workspace_members in cargo metadata")?;

    let mut violations = Vec::new();

    for member in workspace_members {
        let member_id = member.as_str().unwrap_or_default();
        let Some(pkg) = packages.iter().find(|p| p.get("id").and_then(Value::as_str) == Some(member_id)) else {
            continue;
        };

        let pkg_name = pkg.get("name").and_then(Value::as_str).unwrap_or_default();
        let short_name = pkg_name.strip_prefix("encodercraft-").unwrap_or(pkg_name);
        let Some(&(_, src_class)) = TABLE.iter().find(|(name, _)| *name == short_name) else {
            violations.push(format!("package `{pkg_name}` is not listed in xtask layers TABLE"));
            continue;
        };

        let Some(src_layer) = src_class.layer() else {
            continue; // Exempt
        };

        let deps = pkg.get("dependencies").and_then(Value::as_array).cloned().unwrap_or_default();
        for dep in deps {
            // Ignore dev-dependencies
            if dep.get("kind").and_then(Value::as_str) == Some("dev") {
                continue;
            }

            let dep_name = dep.get("name").and_then(Value::as_str).unwrap_or_default();
            let dep_short = dep_name.strip_prefix("encodercraft-").unwrap_or(dep_name);

            // Check if dependency is within our workspace
            if let Some(&(_, target_class)) = TABLE.iter().find(|(name, _)| *name == dep_short) {
                if src_class == Class::Standalone {
                    violations.push(format!("layer violation: standalone crate `{pkg_name}` cannot depend on workspace crate `{dep_name}`"));
                } else if let Some(target_layer) = target_class.layer()
                    && target_layer > src_layer
                {
                    violations.push(format!("layer violation: `{pkg_name}` (L{src_layer}) depends on higher layer `{dep_name}` (L{target_layer})"));
                }
            }
        }
    }

    if violations.is_empty() { Ok(()) } else { Err(violations.join("\n")) }
}
