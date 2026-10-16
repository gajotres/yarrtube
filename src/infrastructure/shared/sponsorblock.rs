use anyhow::{Result, bail};

/// A single, optional global removal policy. yt-dlp owns segment lookup and cuts.
#[derive(Clone, Debug, Default)]
pub struct SponsorBlockConfig {
    categories: Option<String>,
}

impl SponsorBlockConfig {
    /// Empty disables removal; `default`, `all`, or an explicit category list enables it.
    pub fn parse(value: &str) -> Result<Self> {
        let value = value.trim();
        if value.is_empty() {
            return Ok(Self::default());
        }
        let categories: Vec<_> = value.split(',').map(str::trim).collect();
        const ALLOWED: &[&str] = &[
            "sponsor",
            "intro",
            "outro",
            "selfpromo",
            "preview",
            "filler",
            "interaction",
            "music_offtopic",
            "hook",
        ];
        if value != "default"
            && value != "all"
            && categories
                .iter()
                .any(|category| !ALLOWED.contains(category))
        {
            bail!(
                "YARRTUBE_SPONSORBLOCK_REMOVE must be empty, default, all, or a comma-separated list of removable SponsorBlock categories"
            );
        }
        Ok(Self {
            categories: Some(categories.join(",")),
        })
    }

    pub fn categories(&self) -> Option<&str> {
        self.categories.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_disabled_presets_and_explicit_categories() {
        assert_eq!(SponsorBlockConfig::parse(" ").unwrap().categories(), None);
        for preset in ["default", "all"] {
            assert_eq!(
                SponsorBlockConfig::parse(preset).unwrap().categories(),
                Some(preset)
            );
        }
        assert_eq!(
            SponsorBlockConfig::parse("sponsor, selfpromo, interaction")
                .unwrap()
                .categories(),
            Some("sponsor,selfpromo,interaction")
        );
    }

    #[test]
    fn rejects_invalid_and_non_removable_categories() {
        for value in [
            "unknown",
            "sponsor,",
            "chapter",
            "poi_highlight",
            "--exec",
            "all,sponsor",
        ] {
            assert!(
                SponsorBlockConfig::parse(value).is_err(),
                "accepted {value}"
            );
        }
    }
}
