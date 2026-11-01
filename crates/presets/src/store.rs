//! Custom preset storage and discovery.

use std::fs::File;
use std::path::PathBuf;

use encodercraft_core::EncoderError;

use crate::factory::factory_presets;
use crate::model::EncodingPreset;

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

            let filename = format!("{}.json", preset.name.to_ascii_lowercase().replace(' ', "-"));
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
