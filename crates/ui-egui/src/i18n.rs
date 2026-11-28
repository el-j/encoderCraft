//! Interface translations through craft-i18n. Strings in code stay English and are the lookup keys; the catalogs map
//! them to display text where they are drawn. Command ids, menu command parameters, preference values, the CLI, MCP and
//! automation never see translated text. A string without a translation shows in English, so coverage grows in steps
//! and a missing translation never fails a build, a test or a release.
//!
//! The catalogs live in [storytold/craft-i18n](https://github.com/storytold/craft-i18n) (`locales/<code>/common.tsv`,
//! shared by the Craft apps, and `encodercraft.tsv`), not in this repository. `build.rs` embeds them from
//! `CRAFT_I18N_DIR`; without it the build is English only. Lookup, fallback (encoderCraft's rows over shared ones),
//! language names and the system language are the `craft-i18n` crate's. A feature PR here adds the English only;
//! translations are added in craft-i18n (AGENTS.md).
//!
//! - [`tl!`] / [`t`]: a UI string in the language the interface is drawn in ([`set_current`]).
//! - [`tid`]: a command's label, keyed by its command id with the English label as fallback.

use std::cell::Cell;
use std::sync::{Arc, LazyLock};

use craft_i18n::{Direction, I18n, Lang};

include!(concat!(env!("OUT_DIR"), "/craft_i18n.rs"));

/// The `language` preference value that follows the operating system's language.
pub const SYSTEM: &str = "auto";

/// The embedded catalogs. English and every embedded language written left to right are offered: encoderCraft's layout
/// is not mirrored for right-to-left languages yet. A locale is parsed on its first lookup.
static I18N: LazyLock<Arc<I18n>> = LazyLock::new(|| {
    let mut offered = vec!["en"];
    offered.extend(CRAFT_I18N.iter().map(|(code, ..)| *code).filter(|code| craft_i18n::direction(code) == Direction::Ltr));
    let (i18n, problems) = I18n::new(CRAFT_I18N, &offered);
    for problem in problems {
        log::warn!("interface catalogs: {problem}");
    }
    Arc::new(i18n)
});

thread_local! {
    /// The language the interface is drawn in. Per thread, so a test that switches it leaves other tests alone.
    static CURRENT: Cell<Lang> = const { Cell::new(Lang::EN) };
}

/// Translate a UI string into the language the interface is drawn in.
macro_rules! tl {
    ($s:expr) => {
        $crate::i18n::t($s)
    };
}
pub(crate) use tl;

/// The language the `language` preference asks for: [`SYSTEM`], or a code this build doesn't offer, follows the
/// operating system's language (English when the system's isn't offered).
pub fn resolve(preference: &str) -> Lang {
    match preference {
        SYSTEM => I18N.system_lang(),
        code => I18N.from_code(code).unwrap_or_else(|| I18N.system_lang()),
    }
}

/// Draw the interface in `lang` from now on (on this thread).
pub fn set_current(lang: Lang) {
    CURRENT.with(|current| current.set(lang));
}

/// The language the interface is drawn in.
pub fn current() -> Lang {
    CURRENT.with(Cell::get)
}

/// `source` in the language the interface is drawn in; `source` itself when there is no translation.
pub fn t(source: &str) -> &str {
    I18N.t(current(), source)
}

/// `template` in the interface language with its `{name}` placeholders filled from `args`: the [`Translate`] that
/// craft-update's updater words its messages with.
///
/// [`Translate`]: craft_update::translate::Translate
pub fn fill(template: &str, args: &[(&str, &str)]) -> String {
    craft_i18n::fmt(t(template), args)
}

/// The embedded catalogs, shared: what the craft-ui host words its built-in text with.
pub fn shared() -> Arc<I18n> {
    Arc::clone(&I18N)
}

/// The label of the command `id` in the language the interface is drawn in; `english` when there is no translation.
pub fn tid<'a>(id: &str, english: &'a str) -> &'a str {
    I18N.tid(current(), id, english)
}

/// A command's label in the interface language, wherever it is shown (menus, the shortcut list, messages): the
/// command table's English label with `...` written as `…`, the form the menus show and the catalogs key on, looked
/// up by the command's id.
pub fn command_label(id: &str, english: &str) -> String {
    let english = english.replace("...", "…");
    tid(id, &english).to_owned()
}

/// The languages this build offers, English first: each one's `language` preference value and its name in itself.
pub fn languages() -> Vec<(&'static str, &'static str)> {
    let i18n: &'static I18n = &I18N;
    i18n.languages().into_iter().map(|lang| (i18n.code(lang), i18n.name(lang))).collect()
}

/// The `language` preference value of `lang` ("en", "ja").
pub fn code(lang: Lang) -> &'static str {
    let i18n: &'static I18n = &I18N;
    i18n.code(lang)
}

/// The name of `lang` in itself ("English", "日本語").
pub fn name(lang: Lang) -> &'static str {
    let i18n: &'static I18n = &I18N;
    i18n.name(lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_this_build_does_not_offer_follows_the_system_like_auto() {
        assert_eq!(resolve("xx-not-a-language"), resolve(SYSTEM));
        assert_eq!(resolve(""), resolve(SYSTEM));
    }

    #[test]
    fn command_labels_use_the_catalog_form_of_an_ellipsis() {
        for info in crate::commands::COMMANDS.iter().filter(|info| info.label.ends_with("...")) {
            let label = command_label(info.id, info.label);
            assert!(!label.contains("...") && label.ends_with('…'), "{}: {label:?}", info.id);
        }
    }

    #[test]
    fn every_offered_language_is_a_preference_that_selects_it() {
        let offered = languages();
        assert_eq!(offered.first().map(|(code, _)| *code), Some("en"), "English is always offered, first");
        for (code, _) in offered {
            assert_eq!(super::code(resolve(code)), code);
        }
    }
}
