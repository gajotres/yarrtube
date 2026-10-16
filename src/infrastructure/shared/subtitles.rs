use anyhow::{Result, bail};

/// Subtitle downloads are opt-in; embedding of downloaded SRTs defaults to on.
#[derive(Clone, Debug)]
pub struct SubtitleConfig {
    pub write_subs: bool,
    pub write_auto_subs: bool,
    pub embed: bool,
    languages: String,
}

impl Default for SubtitleConfig {
    fn default() -> Self {
        Self {
            write_subs: false,
            write_auto_subs: false,
            embed: true,
            languages: "en.*".into(),
        }
    }
}

impl SubtitleConfig {
    pub fn parse(creator: &str, automatic: &str, languages: &str) -> Result<Self> {
        fn flag(value: &str, name: &str) -> Result<bool> {
            match value.trim() {
                "" | "false" => Ok(false),
                "true" => Ok(true),
                _ => bail!("{name} must be true or false"),
            }
        }
        let languages = if languages.trim().is_empty() {
            "en.*"
        } else {
            languages
        };
        let parts: Vec<_> = languages.split(',').map(str::trim).collect();
        if parts.iter().any(|part| {
            part.is_empty()
                || part.starts_with("--")
                || part.chars().any(|c| c.is_whitespace() || c.is_control())
        }) {
            bail!(
                "YARRTUBE_SUB_LANGS must be comma-separated language codes or patterns, e.g. en.*,de.*"
            );
        }
        Ok(Self {
            write_subs: flag(creator, "YARRTUBE_WRITE_SUBS")?,
            write_auto_subs: flag(automatic, "YARRTUBE_WRITE_AUTO_SUBS")?,
            languages: parts.join(","),
            embed: true,
        })
    }

    pub fn with_embedding(mut self, value: &str) -> Result<Self> {
        self.embed = match value.trim() {
            "" | "true" => true,
            "false" => false,
            _ => bail!("YARRTUBE_EMBED_SUBS must be true or false"),
        };
        Ok(self)
    }

    pub fn append_args(&self, args: &mut Vec<String>) {
        args.push(
            if self.write_subs {
                "--write-subs"
            } else {
                "--no-write-subs"
            }
            .into(),
        );
        args.push(
            if self.write_auto_subs {
                "--write-auto-subs"
            } else {
                "--no-write-auto-subs"
            }
            .into(),
        );
        // Native embedding must stay off: Yarrtube embeds after repairing cut timing.
        args.push("--no-embed-subs".into());
        if self.write_subs || self.write_auto_subs {
            args.extend([
                "--sub-langs".into(),
                format!("{},-live_chat", self.languages),
                "--sub-format".into(),
                "srt/vtt/best".into(),
                "--convert-subs".into(),
                "srt".into(),
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_independent_caption_switches_without_native_embedding() {
        for (creator, automatic) in [
            ("false", "false"),
            ("true", "false"),
            ("false", "true"),
            ("true", "true"),
        ] {
            let config = SubtitleConfig::parse(creator, automatic, "en.*, de.*").unwrap();
            let mut args = Vec::new();
            config.append_args(&mut args);
            assert!(
                args.contains(
                    &if creator == "true" {
                        "--write-subs"
                    } else {
                        "--no-write-subs"
                    }
                    .to_string()
                )
            );
            assert!(
                args.contains(
                    &if automatic == "true" {
                        "--write-auto-subs"
                    } else {
                        "--no-write-auto-subs"
                    }
                    .to_string()
                )
            );
            assert!(args.contains(&"--no-embed-subs".to_string()));
            assert!(!args.contains(&"--embed-subs".to_string()));
            assert_eq!(
                args.contains(&"en.*,de.*,-live_chat".to_string()),
                creator == "true" || automatic == "true"
            );
        }
    }

    #[test]
    fn defaults_to_disabled_and_english_and_rejects_invalid_configuration() {
        let config = SubtitleConfig::parse("", "", "").unwrap();
        assert!(!config.write_subs && !config.write_auto_subs);
        assert_eq!(config.languages, "en.*");
        assert!(config.embed);
        assert!(!config.clone().with_embedding("false").unwrap().embed);
        assert!(config.clone().with_embedding("").unwrap().embed);
        assert!(config.with_embedding("yes").is_err());
        for values in [
            ("yes", "false", "en"),
            ("true", "1", "en"),
            ("true", "true", "en,"),
            ("true", "false", "--exec"),
            ("true", "true", "en\nde"),
        ] {
            assert!(SubtitleConfig::parse(values.0, values.1, values.2).is_err());
        }
    }
}
