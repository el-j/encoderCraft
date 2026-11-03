//! Custom preset storage and discovery.

use std::fs::File;
use std::path::PathBuf;

use encodercraft_core::EncoderError;

use crate::factory::factory_presets;
use crate::model::EncodingPreset;

/// File name for a preset; only ASCII letters and digits survive so a name can never escape the directory.
fn file_name(name: &str) -> String {
    let stem: String = name.to_ascii_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    format!("{stem}.json")
}

/// Combined registry of factory presets and user custom presets.
#[derive(Debug, Clone)]
pub struct PresetRegistry {
    presets: Vec<EncodingPreset>,
    custom_dir: Option<PathBuf>,
}

impl Default for PresetRegistry {
    fn default() -> Self {
        Self::new(None)
    }
}

impl PresetRegistry {
    /// Creates a preset registry initialized with factory presets and optional user custom preset directory.
    pub fn new(custom_dir: Option<PathBuf>) -> Self {
        let mut reg = Self { presets: factory_presets(), custom_dir };
        let _ = reg.reload_custom_presets();
        reg
    }

    /// Finds a preset by its exact or case-insensitive name.
    pub fn find_by_name(&self, name: &str) -> Option<&EncodingPreset> {
        let needle = name.trim().to_ascii_lowercase();
        self.presets.iter().find(|p| p.name.to_ascii_lowercase() == needle)
    }

    /// Returns a list of all currently registered presets.
    pub fn list(&self) -> &[EncodingPreset] {
        &self.presets
    }

    /// Adds a user preset to the registry and writes it to disk if custom_dir is set.
    pub fn save_custom_preset(&mut self, preset: EncodingPreset) -> Result<(), EncoderError> {
        if let Some(ref dir) = self.custom_dir {
            std::fs::create_dir_all(dir).map_err(|e| EncoderError::Io(format!("failed to create presets directory: {e}")))?;

            let filename = file_name(&preset.name);
            let file_path = dir.join(filename);

            let file = File::create(&file_path).map_err(|e| EncoderError::Io(format!("failed to create preset file: {e}")))?;
            serde_json::to_writer_pretty(file, &preset).map_err(|e| EncoderError::Internal(format!("failed to serialize preset: {e}")))?;
        }

        // Replace if already exists or append
        if let Some(pos) = self.presets.iter().position(|p| p.name == preset.name) {
            self.presets[pos] = preset;
        } else {
            self.presets.push(preset);
        }

        Ok(())
    }

    /// True for presets shipped with the application (they cannot be deleted).
    pub fn is_factory(&self, name: &str) -> bool {
        factory_presets().iter().any(|p| p.name == name)
    }

    /// Removes a user preset from the registry and from disk. Factory presets are protected.
    pub fn delete_custom_preset(&mut self, name: &str) -> Result<(), EncoderError> {
        if self.is_factory(name) {
            return Err(EncoderError::Internal(format!("'{name}' is a built-in preset and cannot be deleted")));
        }
        let Some(pos) = self.presets.iter().position(|p| p.name == name) else {
            return Err(EncoderError::Internal(format!("no preset named '{name}'")));
        };
        if let Some(ref dir) = self.custom_dir {
            let file = dir.join(file_name(name));
            if file.exists() {
                std::fs::remove_file(&file).map_err(|e| EncoderError::Io(format!("failed to delete preset file: {e}")))?;
            }
        }
        self.presets.remove(pos);
        Ok(())
    }

    /// Imports presets from a bundle. A name that collides with a built-in preset gets an " (imported)" suffix.
    /// Returns how many presets were added or updated.
    pub fn import_presets(&mut self, presets: Vec<EncodingPreset>) -> Result<usize, EncoderError> {
        let mut count = 0;
        for mut p in presets {
            p.name = p.name.trim().to_string();
            if self.is_factory(&p.name) {
                p.name = format!("{} (imported)", p.name);
            }
            self.save_custom_preset(p)?;
            count += 1;
        }
        Ok(count)
    }

    /// Reloads all user custom presets from the custom preset directory.
    pub fn reload_custom_presets(&mut self) -> Result<(), EncoderError> {
        if let Some(ref dir) = self.custom_dir {
            if !dir.exists() {
                return Ok(());
            }

            let entries = std::fs::read_dir(dir).map_err(|e| EncoderError::Io(format!("failed to read custom presets dir: {e}")))?;

            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json")
                    && let Ok(file) = File::open(&path)
                    && let Ok(preset) = serde_json::from_reader::<_, EncodingPreset>(file)
                    && !self.presets.iter().any(|p| p.name == preset.name)
                {
                    self.presets.push(preset);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn import_renames_factory_collisions_and_delete_protects_factory() {
        let dir = tempdir().unwrap();
        let mut reg = PresetRegistry::new(Some(dir.path().to_path_buf()));
        let factory = reg.list()[0].clone();
        let mut custom = factory.clone();
        custom.name = "Shared One".into();
        let n = reg.import_presets(vec![factory.clone(), custom]).unwrap();
        assert_eq!(n, 2);
        assert!(reg.find_by_name(&format!("{} (imported)", factory.name)).is_some());
        assert!(reg.delete_custom_preset(&factory.name).is_err());
        reg.delete_custom_preset("Shared One").unwrap();
        assert!(reg.find_by_name("Shared One").is_none());
        assert!(!dir.path().join("shared-one.json").exists());
        assert!(reg.delete_custom_preset("Shared One").is_err());
    }

    #[test]
    fn hostile_names_cannot_escape_the_preset_directory() {
        let dir = tempdir().unwrap();
        let inner = dir.path().join("presets");
        let mut reg = PresetRegistry::new(Some(inner.clone()));
        let mut p = reg.list()[0].clone();
        p.name = "../../escape".into();
        reg.save_custom_preset(p).unwrap();
        assert!(inner.join("------escape.json").exists());
        assert!(!dir.path().join("escape.json").exists());
    }

    #[test]
    fn test_preset_registry_custom_save() {
        let dir = tempdir().unwrap();
        let mut reg = PresetRegistry::new(Some(dir.path().to_path_buf()));

        let custom = EncodingPreset {
            name: "My Custom Preset".into(),
            description: "Custom".into(),
            category: crate::model::PresetCategory::Custom,
            container: encodercraft_core::ContainerType::IsoBmffMp4,
            video: None,
            audio: None,
        };

        reg.save_custom_preset(custom).unwrap();
        assert!(reg.find_by_name("My Custom Preset").is_some());
        assert!(dir.path().join("my-custom-preset.json").exists());
    }
}
