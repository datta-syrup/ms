//! The cast's voices made on an ElevenLabs account, against a stand-in
//! server on this machine through the same `curl` the real calls use: a
//! voice designed from the character's description and sample, its first
//! take kept with the character's name and label; a voice-design model the
//! account can't use giving way to the next; labels the account won't take
//! left off; a refused key saying why; and the voice found again, by its
//! label, once the account lists it.

use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use ms::ai::cast::Cast;
use ms::ai::eleven::Eleven;
use ms::phone::http::{Conn, Response};
use serde_json::{Value, json};

const KEY: &str = "sk_0123456789abcdef0123456789abcdef0123456789abcdef";

/// How the stand-in account behaves.
#[derive(Clone, Copy, Default)]
struct Account {
    /// The first voice-design model is not available to it.
    no_first_model: bool,
    /// It won't take labels on a voice.
    no_labels: bool,
}

/// A fake ElevenLabs: every request kept (path, output format, body), the
/// voices it made listed back.
fn fake(cast: &Cast, account: Account) -> (String, Arc<Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let made: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let first_model = cast.design_models[0].clone();
    let log = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (log, made, first_model) =
                (Arc::clone(&log), Arc::clone(&made), first_model.clone());
            std::thread::spawn(move || {
                let mut conn = Conn::new(stream);
                while let Ok(Some(req)) = conn.read_request(1 << 20) {
                    let body: Value = serde_json::from_slice(&req.body).unwrap_or(Value::Null);
                    log.lock().unwrap().push(json!({
                        "path": req.path,
                        "format": req.param("output_format"),
                        "body": body,
                    }));
                    let response = if req.header("xi-api-key") != Some(KEY) {
                        Response::json(
                            401,
                            &json!({"detail": {"status": "invalid_api_key", "message": "Invalid API key"}}),
                        )
                    } else {
                        match req.path.as_str() {
                            "/v1/text-to-voice/design"
                                if account.no_first_model
                                    && body["model_id"] == first_model.as_str() =>
                            {
                                Response::json(
                                    422,
                                    &json!({"detail": {"status": "invalid_model", "message": "This model is not available to you."}}),
                                )
                            }
                            "/v1/text-to-voice/design" => Response::json(
                                200,
                                &json!({
                                    "previews": [
                                        {"generated_voice_id": "take-1", "audio_base_64": "", "media_type": "audio/mpeg", "duration_secs": 9.5},
                                        {"generated_voice_id": "take-2", "audio_base_64": "", "media_type": "audio/mpeg", "duration_secs": 9.1},
                                    ],
                                    "text": body["text"],
                                }),
                            ),
                            "/v1/text-to-voice"
                                if account.no_labels && body.get("labels").is_some() =>
                            {
                                Response::json(
                                    422,
                                    &json!({"detail": [{"loc": ["body", "labels"], "msg": "Extra inputs are not permitted", "type": "extra_forbidden"}]}),
                                )
                            }
                            "/v1/text-to-voice" => {
                                let voice = json!({
                                    "voice_id": format!("made-{}", body["generated_voice_id"].as_str().unwrap_or("?")),
                                    "name": body["voice_name"],
                                    "labels": body.get("labels").cloned().unwrap_or(json!({})),
                                    "category": "generated",
                                });
                                made.lock().unwrap().push(voice.clone());
                                Response::json(200, &voice)
                            }
                            "/v1/voices" => {
                                Response::json(200, &json!({"voices": *made.lock().unwrap()}))
                            }
                            _ => Response::json(404, &json!({"detail": "Not Found"})),
                        }
                    };
                    if conn.write_response(&response, !req.wants_close()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

fn have_curl() -> bool {
    std::process::Command::new(if cfg!(windows) { "curl.exe" } else { "curl" })
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn asked(seen: &Mutex<Vec<Value>>, path: &str) -> Vec<Value> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|r| r["path"] == path)
        .cloned()
        .collect()
}

#[test]
fn a_characters_voice_is_designed_from_its_description_and_kept_with_its_label() {
    if !have_curl() {
        eprintln!("curl is not installed here; skipped");
        return;
    }
    let cast = Cast::built_in().unwrap();
    let (base, seen) = fake(&cast, Account::default());
    let eleven = Eleven::new(KEY, &base);
    let character = &cast.characters[0];

    // Not on the account yet.
    let listed = eleven.voices().unwrap();
    assert!(cast.voice_of(character, &listed).is_none());

    let voice = cast.make(character, &eleven).unwrap();
    // Designed from how it sounds, saying its sample, with the first model,
    // the takes sent back small.
    let design = asked(&seen, "/v1/text-to-voice/design");
    assert_eq!(design.len(), 1);
    assert_eq!(
        design[0]["body"]["voice_description"],
        character.voice.as_str()
    );
    assert_eq!(design[0]["body"]["text"], character.sample.as_str());
    assert_eq!(
        design[0]["body"]["model_id"],
        cast.design_models[0].as_str()
    );
    assert!(design[0]["format"].as_str().is_some_and(|f| !f.is_empty()));
    // The first take kept, under the character's name and with its label.
    let save = asked(&seen, "/v1/text-to-voice");
    assert_eq!(save.len(), 1);
    assert_eq!(save[0]["body"]["generated_voice_id"], "take-1");
    assert_eq!(save[0]["body"]["voice_name"], cast.voice_name_of(character));
    assert_eq!(
        save[0]["body"]["voice_description"],
        character.voice.as_str()
    );
    assert_eq!(
        save[0]["body"]["labels"],
        json!({cast.label.clone(): character.id.clone()})
    );
    assert_eq!(voice.id, "made-take-1");
    assert_eq!(voice.name, cast.voice_name_of(character));

    // The account lists it now: found again by its label, not made twice.
    let listed = eleven.voices().unwrap();
    assert_eq!(
        cast.voice_of(character, &listed).map(|v| v.id.as_str()),
        Some("made-take-1")
    );
    assert!(cast.voice_of(&cast.characters[1], &listed).is_none());
}

#[test]
fn a_model_the_account_cant_use_gives_way_and_refused_labels_are_left_off() {
    if !have_curl() {
        return;
    }
    let cast = Cast::built_in().unwrap();
    assert!(cast.design_models.len() > 1, "a model to fall back on");
    let (base, seen) = fake(
        &cast,
        Account {
            no_first_model: true,
            no_labels: true,
        },
    );
    let eleven = Eleven::new(KEY, &base);
    let character = &cast.characters[1];
    let voice = cast.make(character, &eleven).unwrap();

    let models: Vec<Value> = asked(&seen, "/v1/text-to-voice/design")
        .iter()
        .map(|r| r["body"]["model_id"].clone())
        .collect();
    assert_eq!(
        models,
        [json!(cast.design_models[0]), json!(cast.design_models[1])]
    );
    // Saved again without the labels: the voice is made all the same, and
    // found by its name.
    let save = asked(&seen, "/v1/text-to-voice");
    assert_eq!(save.len(), 2);
    assert!(save[0]["body"].get("labels").is_some());
    assert!(save[1]["body"].get("labels").is_none());
    assert!(voice.labels.is_empty());
    let listed = eleven.voices().unwrap();
    assert_eq!(
        cast.voice_of(character, &listed).map(|v| v.id.as_str()),
        Some("made-take-1")
    );
}

#[test]
fn a_refused_key_says_why_and_makes_nothing() {
    if !have_curl() {
        return;
    }
    let cast = Cast::built_in().unwrap();
    let (base, seen) = fake(&cast, Account::default());
    let refused = cast
        .make(&cast.characters[0], &Eleven::new("sk_refused", &base))
        .unwrap_err();
    assert!(
        refused.detail().contains("invalid_api_key"),
        "{}",
        refused.detail()
    );
    assert!(asked(&seen, "/v1/text-to-voice").is_empty());
}
