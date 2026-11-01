//! The cast of MapleSyrup's story ("One, the boy who stepped on snails"):
//! characters the player can pick as MapleSyrup's voice on the phone. Who
//! they are lives in [`CAST`] (`assets/voices/cast.json`), not here: each
//! one's name and a few words about it in the languages it has, how it
//! sounds, what it says while its voice is made, and what it says once it
//! is MapleSyrup's voice.
//!
//! A character's voice is made on the player's own ElevenLabs account the
//! first time it is picked ([`Eleven::design`]: a voice from a description,
//! no one's recording), named after it and carrying the cast's label, and
//! found there again from then on — by that label, or by its name on an
//! account that kept no labels. Only the characters picked take up the
//! account's voices and credits.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::eleven::{DESIGN_TEXT, Design, Eleven, Voice};
use super::openai::AiError;

/// The cast as it ships, built into the program.
pub const CAST: &str = include_str!("../../assets/voices/cast.json");

/// What stands for a character's name in [`Cast::voice_name`].
pub const NAME_SLOT: &str = "{name}";

/// The longest id the phone may send back (a voice's or a character's).
pub const ID_MAX: usize = 64;

/// The characters, and how their voices are made and found.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Cast {
    /// The language every name and line is in, for a phone whose language
    /// a character doesn't have.
    pub fallback: String,
    /// The label a character's voice carries on the account; its value is
    /// the character's id.
    pub label: String,
    /// The name a character's voice gets on the account, [`NAME_SLOT`]
    /// standing for the character's name.
    pub voice_name: String,
    /// ElevenLabs's voice-design models, tried in order.
    pub design_models: Vec<String>,
    pub characters: Vec<Character>,
}

/// One of the characters.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Character {
    /// How the program and the phone know it.
    pub id: String,
    /// Its name, by language.
    pub name: BTreeMap<String, String>,
    /// A few words about it, by language.
    pub about: BTreeMap<String, String>,
    /// How it sounds: its voice is made from this.
    pub voice: String,
    /// What it says while its voice is made.
    pub sample: String,
    /// What it says once it is MapleSyrup's voice (translated for the
    /// player, like MapleSyrup's other lines).
    pub hello: String,
}

impl Cast {
    /// The cast built into the program.
    pub fn built_in() -> Result<Cast, String> {
        Cast::parse(CAST)
    }

    /// A cast from its JSON, checked: each character has an id the phone
    /// can send, its own; a name and a few words in the fallback language;
    /// how it sounds; what it says while it is made, as long as ElevenLabs
    /// allows; and what it says once it is the voice.
    pub fn parse(text: &str) -> Result<Cast, String> {
        let cast: Cast = serde_json::from_str(text).map_err(|e| e.to_string())?;
        cast.check()?;
        Ok(cast)
    }

    fn check(&self) -> Result<(), String> {
        let blank = |s: &str| s.trim().is_empty();
        if blank(&self.fallback) || blank(&self.label) {
            return Err("the cast needs a fallback language and a label".into());
        }
        if !self.voice_name.contains(NAME_SLOT) {
            return Err(format!("the voices' name must hold {NAME_SLOT}"));
        }
        if self.design_models.iter().all(|m| blank(m)) {
            return Err("the cast needs a voice-design model".into());
        }
        let mut ids = BTreeSet::new();
        for c in &self.characters {
            if !valid_id(&c.id) {
                return Err(format!("{:?}: not an id the phone can send", c.id));
            }
            if !ids.insert(c.id.as_str()) {
                return Err(format!("{}: in the cast twice", c.id));
            }
            for (what, by_language) in [("name", &c.name), ("about", &c.about)] {
                if by_language.get(&self.fallback).is_none_or(|s| blank(s)) {
                    return Err(format!("{}: no {what} in {}", c.id, self.fallback));
                }
            }
            if blank(&c.voice) || blank(&c.hello) {
                return Err(format!("{}: how it sounds and its hello are needed", c.id));
            }
            let length = c.sample.chars().count();
            if !DESIGN_TEXT.contains(&length) {
                return Err(format!(
                    "{}: its sample is {length} characters, not {}–{}",
                    c.id,
                    DESIGN_TEXT.start(),
                    DESIGN_TEXT.end()
                ));
            }
        }
        Ok(())
    }

    pub fn character(&self, id: &str) -> Option<&Character> {
        self.characters.iter().find(|c| c.id == id)
    }

    /// A character's name in the fallback language.
    pub fn name_of<'a>(&self, character: &'a Character) -> &'a str {
        character
            .name
            .get(&self.fallback)
            .map(String::as_str)
            .unwrap_or(&character.id)
    }

    /// The name a character's voice has on the account.
    pub fn voice_name_of(&self, character: &Character) -> String {
        self.voice_name.replace(NAME_SLOT, self.name_of(character))
    }

    /// The voice on the account that is this character's: the one with its
    /// label, else (an account that kept no labels) the one with its name.
    pub fn voice_of<'a>(&self, character: &Character, voices: &'a [Voice]) -> Option<&'a Voice> {
        voices
            .iter()
            .find(|v| v.labels.get(&self.label) == Some(&character.id))
            .or_else(|| {
                let name = self.voice_name_of(character);
                voices.iter().find(|v| v.name == name)
            })
    }

    /// Makes `character`'s voice on the account behind `eleven`.
    pub fn make(&self, character: &Character, eleven: &Eleven) -> Result<Voice, AiError> {
        let name = self.voice_name_of(character);
        let labels = BTreeMap::from([(self.label.clone(), character.id.clone())]);
        eleven.design(&Design {
            description: &character.voice,
            text: &character.sample,
            models: &self.design_models,
            name: &name,
            labels: &labels,
        })
    }

    /// The cast for the phone: each character's name and few words in the
    /// languages it has, and its voice on the account (`voices`) when it has
    /// one; and the character whose voice is being made.
    pub fn status(&self, voices: &[Voice], making: Option<&str>) -> Value {
        let characters: Vec<Value> = self
            .characters
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "name": c.name,
                    "about": c.about,
                    "voice": self.voice_of(c, voices).map(|v| v.id.as_str()),
                })
            })
            .collect();
        json!({
            "fallback": self.fallback,
            "making": making,
            "characters": characters,
        })
    }
}

/// An id the phone may send back: a voice's or a character's.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cast() -> Cast {
        Cast::built_in().expect("the cast as it ships")
    }

    #[test]
    fn the_cast_as_it_ships_is_whole() {
        let cast = cast();
        assert!(!cast.characters.is_empty());
        // Each language one character has, they all have: none is left in
        // the fallback language on a phone in another.
        let languages: BTreeSet<&String> = cast
            .characters
            .iter()
            .flat_map(|c| c.name.keys().chain(c.about.keys()))
            .collect();
        for c in &cast.characters {
            for language in &languages {
                assert!(
                    c.name.contains_key(*language),
                    "{}: name in {language}",
                    c.id
                );
                assert!(
                    c.about.contains_key(*language),
                    "{}: about in {language}",
                    c.id
                );
            }
            // Every voice has its own name on the account.
            let name = cast.voice_name_of(c);
            assert!(name.contains(cast.name_of(c)) && !name.contains(NAME_SLOT));
        }
        let names: BTreeSet<String> = cast
            .characters
            .iter()
            .map(|c| cast.voice_name_of(c))
            .collect();
        assert_eq!(names.len(), cast.characters.len());
    }

    /// The cast as it ships with one thing changed in its JSON.
    fn changed(change: impl FnOnce(&mut Value)) -> Result<Cast, String> {
        let mut json: Value = serde_json::from_str(CAST).unwrap();
        change(&mut json);
        Cast::parse(&json.to_string())
    }

    #[test]
    fn a_broken_cast_says_what_is_wrong() {
        let first = |json: &mut Value| json["characters"][0].clone();
        for (change, says) in [
            (
                Box::new(|j: &mut Value| j["characters"][0]["id"] = json!("../up"))
                    as Box<dyn FnOnce(&mut Value)>,
                "not an id",
            ),
            (
                Box::new(move |j: &mut Value| {
                    let again = first(j);
                    j["characters"].as_array_mut().unwrap().push(again);
                }),
                "twice",
            ),
            (
                Box::new(|j: &mut Value| j["characters"][0]["sample"] = json!("Too short.")),
                "sample",
            ),
            (
                Box::new(|j: &mut Value| {
                    let fallback = j["fallback"].as_str().unwrap().to_string();
                    j["characters"][0]["name"]
                        .as_object_mut()
                        .unwrap()
                        .remove(&fallback);
                }),
                "no name",
            ),
            (
                Box::new(|j: &mut Value| j["voice_name"] = json!("MapleSyrup")),
                NAME_SLOT,
            ),
            (
                Box::new(|j: &mut Value| j["design_models"] = json!([])),
                "model",
            ),
            (
                Box::new(|j: &mut Value| j["characters"][0]["hello"] = json!(" ")),
                "hello",
            ),
        ] {
            let why = changed(change).unwrap_err();
            assert!(why.contains(says), "{why}");
        }
        assert!(changed(|_| {}).is_ok());
    }

    #[test]
    fn a_characters_voice_is_found_by_its_label_or_else_by_its_name() {
        let cast = cast();
        let (first, second) = (&cast.characters[0], &cast.characters[1]);
        let labelled = Voice {
            id: "v-labelled".into(),
            name: "Renamed by the player".into(),
            labels: BTreeMap::from([(cast.label.clone(), first.id.clone())]),
            ..Voice::default()
        };
        let named = Voice {
            id: "v-named".into(),
            name: cast.voice_name_of(second),
            ..Voice::default()
        };
        let other = Voice {
            id: "v-other".into(),
            name: cast.name_of(first).into(),
            ..Voice::default()
        };
        let voices = [other.clone(), labelled.clone(), named.clone()];
        assert_eq!(cast.voice_of(first, &voices), Some(&labelled));
        assert_eq!(cast.voice_of(second, &voices), Some(&named));
        assert_eq!(cast.voice_of(&cast.characters[2], &voices), None);

        // For the phone: names in every language, the voice when there is
        // one, and who is being made.
        let status = cast.status(&voices, Some(&cast.characters[2].id));
        assert_eq!(status["fallback"], cast.fallback);
        assert_eq!(status["making"], cast.characters[2].id);
        let listed = status["characters"].as_array().unwrap();
        assert_eq!(listed.len(), cast.characters.len());
        assert_eq!(listed[0]["voice"], "v-labelled");
        assert_eq!(listed[1]["voice"], "v-named");
        assert!(listed[2]["voice"].is_null());
        assert_eq!(listed[0]["name"], json!(first.name));
        assert!(listed[0].get("sample").is_none() && listed[0].get("voice_description").is_none());
        assert!(cast.status(&[], None)["making"].is_null());
    }

    #[test]
    fn the_phone_may_send_back_only_plain_ids() {
        for id in ["syrup", "nana-ping", "pNInz6obpgDQGcFmaJgB", "a_b"] {
            assert!(valid_id(id), "{id}");
        }
        for id in ["", "../x", "a b", "סירופ", &"x".repeat(ID_MAX + 1)] {
            assert!(!valid_id(id), "{id}");
        }
    }
}
