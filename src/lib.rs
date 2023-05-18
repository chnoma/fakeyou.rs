//! An easy, synchronous API to access FakeYou's AI TTS services
//!
//! **This is an unofficial API with no connection to storyteller.ai**<br/>
//! **An account with <a href="http://www.fakeyou.com">FakeYou</a> is required to use this API.**
//!
//! It has not been tested on lower tiers, and is still missing features.<br/>
//! At the moment, it is the minimum necessary to query voices, categories, and generate audio using them.
//!
//!
//! # Examples
//! Using only two lines of code, it is possible to generate usable audio.<br/>
//! In these examples, we are using a model token which is already known to us.<br/>
//! **These will take some time to finish, due to the API's queue**
//! ```
//! use fakeyou;
//!
//! fn main() {
//!     let fake_you = fakeyou::authenticate("user_name", "password").unwrap();
//!     fake_you.generate_file_from_token("Hello!", "TM:mc2kebvfwr1p", "hello.wav").unwrap();
//! }
//!
//! ```
//!
//! You may also stream the resulting audio directly to an audio playback library, such as `rodio`:
//! ```
//! use std::io::Cursor;
//! use rodio::{Decoder, OutputStream, source::Source, Sink};
//! use fakeyou;
//!
//! fn main() {
//!     // rodio setup
//!     let (_stream, stream_handle) = OutputStream::try_default().unwrap();
//!     let sink = Sink::try_new(&stream_handle).unwrap();
//!
//!     // actual API use
//!     let fake_you = fakeyou::authenticate("user_name", "password").unwrap();
//!     let bytes = fake_you.generate_bytes_from_token("Hello!", "TM:mc2kebvfwr1p").unwrap();
//!
//!     // play resulting audio
//!     let cursor = Cursor::new(bytes);
//!     let decoder = Decoder::new(cursor).unwrap();
//!     sink.append(decoder);
//!     sink.sleep_until_end();
//! }
//! ```
//! # Session token:
//! Once authenticated, your session token is valid for 20 years, so re-authenticating is practically
//! unnecessary, however, re-caching may be necessary using [FakeYouClient::invalidate_cache] to
//! keep the list of [Category] and [Voice] up to date.

use std::time::{Duration};
use std::fs;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use reqwest;
use thiserror::Error;
use serde::{Serialize, Deserialize};
use serde_json;

use uuid::Uuid;


trait SerdeString {
    fn to_string_handled(&self) -> Result<String, Error>;
}

impl SerdeString for serde_json::Value {
    fn to_string_handled(&self) -> Result<String, Error> {
        let string = self.as_str();
        let string = match string {
            Some(string) => { string.to_string() },
            None => { return Err(Error::ImproperResponse) }
        };
        Ok(string)
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid credentials supplied")]
    InvalidCredentials,
    #[error("Undefined HTTP response")]
    UndefinedResponse,
    #[error("Denied due to too many requests")]
    TooManyRequests,
    #[error("Improper response structure")]
    ImproperResponse,
    #[error("Job failed")]
    JobFailed,
    #[error("File read/write error")]
    IOError,
    #[error("Error making request.")]
    ReqwestError(reqwest::Error),
    #[error("Error serializing JSON")]
    SerializationError(serde_json::Error)
}

#[derive(Debug, Deserialize, Serialize)]
struct TtsJobRequest {
    uuid_idempotency_token: String,
    tts_model_token: String,
    inference_text: String
}

#[derive(Debug, Deserialize, Serialize)]
struct TtsJobResponse {
    success: bool,
    inference_job_token: String,
    inference_job_token_type: String
}


/// Representation of a single voice
#[derive(Clone)]
pub struct Voice {
    pub title: String,
    pub model_token: String,
    pub category_tokens: Vec<String>
}

/// Representation of a category of voices
#[derive(Clone)]
pub struct Category {
    pub title: String,
    pub category_token: String,
    pub model_type: String
}

struct TtsJobResult {
    audio_path: String
}

/// Requests a FakeYouClient using a valid username and password
pub fn authenticate(user_name: &str, password: &str) -> Result<FakeYouClient, Error> {

    let client = reqwest::blocking::Client::builder()
        .cookie_store(true)
        .build();
    let client = match client {
        Ok(resp) => resp,
        Err(e) => return Err(Error::ReqwestError(e)),
    };

    // format! does NOT like curly brackets -- i can probably use json! but whatever
    let body = "{\"username_or_email\": \"".to_owned() + user_name + "\", \"password\": \"" + password + "\"}";

    let response = client
        .post("https://api.fakeyou.com/login")
        .header("Content-Type", "application/json")
        .body(body)
        .send();
    let response = match response {
        Ok(resp) => resp,
        Err(e) => return Err(Error::ReqwestError(e)),
    };

    match response.status().as_u16() {
        200 => {
            let json = match response.text() {
                Ok(j) => j,
                Err(e) => return Err(Error::ReqwestError(e)),
            };
            if json != "{\"success\":true}" {
                return Err(Error::InvalidCredentials);
            }
        },
        401 => { return Err(Error::InvalidCredentials); },
        429 => { return Err(Error::TooManyRequests); },
        _ => { return Err(Error::UndefinedResponse); }
    }

    Ok(FakeYouClient::new(client))
}

fn serialize<T:Serialize>(obj: T) -> Result<serde_json::Value, Error>  {
    let val = match serde_json::to_value(&obj) {
        Ok(v) => v,
        Err(e) => return Err(Error::SerializationError(e)),
    };
    Ok(val)
}

/// An authenticated FakeYou client capable of making requests and directly downloading voice samples.
pub struct FakeYouClient {
    client: reqwest::blocking::Client,
    category_cache: Vec<Category>,
    voice_cache: Vec<Voice>,
    cache_generated: DateTime<Utc>
}

impl FakeYouClient {
    fn new(client: reqwest::blocking::Client) -> FakeYouClient {
        let mut client = FakeYouClient{
            client,
            category_cache: Vec::new(),
            voice_cache: Vec::new(),
            cache_generated: Utc::now()
        };
        client.invalidate_cache().expect("failed to build cache");
        client
    }

    /// Create a request and save the resulting .wav to `filename`
    pub fn generate_file(&self, text: &str, voice: &Voice, filename: &str) -> Result<(), Error> {
        self.generate_file_from_token(text, &voice.model_token, filename)
    }

    /// Create a request and return the .wav data as [Bytes]
    pub fn generate_bytes(&self, text: &str, voice: &Voice) -> Result<Bytes, Error> {
        self.generate_bytes_from_token(text, &voice.model_token)
    }

    /// Return a copy of all cached voices
    pub fn list_voices(&self) -> Vec<Voice> {
        self.voice_cache.to_vec()
    }

    /// Return a copy of all cached categories
    pub fn list_categories(&self) -> Vec<Category> {
        self.category_cache.to_vec()
    }

    /// Return a copy of all [Voice] which correspond to a given [Category]
    pub fn list_voices_by_category(&self, category: &Category) -> Vec<Voice> {
        self.list_voices_by_category_token(&category.category_token)
    }

    /// Return a copy of all [Voice] which correspond to a given category token
    pub fn list_voices_by_category_token(&self, category_token: &str) -> Vec<Voice> {
        self.voice_cache.iter()
            .filter(|voice| voice.category_tokens.contains(&category_token.to_string()))
            .cloned()
            .collect()
    }

    /// Create a request and return the .wav data as [Bytes] using a known model token
    pub fn generate_bytes_from_token(&self, text: &str, tts_model_token: &str) -> Result<Bytes, Error> {
        let job = TtsJobRequest{
            uuid_idempotency_token: Uuid::new_v4().to_string(),
            tts_model_token: tts_model_token.to_string(),
            inference_text: text.to_string()
        };
        let job = self.make_tts_job(job)?;
        let job = self.tts_poll(job)?;
        let data = self.get_bytes(&job.audio_path)?;
        Ok(data)
    }

    /// Create a request and save the resulting .wav to `filename` using a known model token
    pub fn generate_file_from_token(&self, text: &str, tts_model_token: &str, filename: &str) -> Result<(), Error> {
        let data = self.generate_bytes_from_token(text, tts_model_token)?;
        let result = fs::write(filename, data);
        let _result = match result {
            Ok(_) => { Ok(()) }
            Err(_) => { Err(Error::IOError) }
        };
        Ok(())
    }

    /// Refresh cache of Voices and Categories
    pub fn invalidate_cache(&mut self) -> Result<(), Error>  {
        let categories_json = self.get_categories()?;
        let mut categories: Vec<Category> = Vec::new();
        let mut voices: Vec<Voice> = Vec::new();

        for object in categories_json["categories"].as_array().unwrap() {
            let category = Category{
                title: object["name"].to_string_handled()?,
                category_token: object["category_token"].to_string_handled()?,
                model_type: object["model_type"].to_string_handled()?
            };
            categories.push(category);
        }
        let voices_json = self.get_voices()?;
        for object in voices_json["models"].as_array().unwrap() {
            // print!("{:#?}", object);
            let mut category_tokens: Vec<String> = Vec::new();
            for category in object["category_tokens"].as_array().unwrap() {
                let category = category.as_str().ok_or(Error::ImproperResponse)?;
                category_tokens.push(category.to_string());
            }
            let voice = Voice{
                title: object["title"].to_string_handled()?,
                model_token: object["model_token"].to_string_handled()?,
                category_tokens
            };
            voices.push(voice);
        }
        self.category_cache = categories;
        self.voice_cache = voices;
        self.cache_generated = Utc::now();
        Ok(())
    }

    fn get_categories(&self) -> Result<serde_json::Value, Error> {
        let resp = self.get_json("https://api.fakeyou.com/category/list/tts")?;
        Ok(resp)
    }

    fn get_voices(&self) -> Result<serde_json::Value, Error> {
        let resp = self.get_json("https://api.fakeyou.com/tts/list")?;
        Ok(resp)
    }

    fn tts_poll(&self, job: TtsJobResponse) -> Result<TtsJobResult, Error> {
        let mut url = "https://api.fakeyou.com/tts/job/".to_string();
        url.push_str(&job.inference_job_token.to_string());
        let data: TtsJobResult;
        loop {
            let resp = self.get_json(&url)?;
            match &resp["state"]["status"] {
                serde_json::Value::String(status) => {
                    match status.as_str() {
                        "started" => {}
                        "pending" => {}
                        "attempt_failed" => {return Err(Error::JobFailed)}
                        "dead" => {return Err(Error::JobFailed)}
                        "complete_success" => {
                            let path = resp["state"]["maybe_public_bucket_wav_audio_path"].to_string_handled()?;
                            data = TtsJobResult{
                                audio_path: "https://storage.googleapis.com/vocodes-public".to_string()
                                    + &path
                            };
                            break;
                        }
                        &_ => {return Err(Error::ImproperResponse)}
                    }
                }
                serde_json::Value::Null => {return Err(Error::ImproperResponse)}
                serde_json::Value::Bool(_) => {return Err(Error::ImproperResponse)}
                serde_json::Value::Number(_) => {return Err(Error::ImproperResponse)}
                serde_json::Value::Array(_) => {return Err(Error::ImproperResponse)}
                serde_json::Value::Object(_) => {return Err(Error::ImproperResponse)}
            }
            std::thread::sleep(Duration::from_secs(2)); // Ensure we do not make too many requests
        }
        Ok(data)
    }

    fn make_tts_job(&self, job_request: TtsJobRequest) -> Result<TtsJobResponse, Error> {
        let response = self.post("https://api.fakeyou.com/tts/inference", job_request)?;
        let json = match response.text() {
            Ok(json) => json,
            Err(e) => return Err(Error::ReqwestError(e))
        };
        let response = serde_json::from_str(&json);
        let response: TtsJobResponse = match response {
            Ok(response) => response,
            Err(e) => return Err(Error::SerializationError(e))
        };
        Ok(response)
    }

    fn get_json(&self, url: &str) -> Result<serde_json::Value, Error> {
        let response = self.get(url)?;
        let json = match response.text() {
            Ok(json) => json,
            Err(e) => return Err(Error::ReqwestError(e)),
        };
        let json = serde_json::from_str(&*json);
        let json = match json {
            Ok(json) => json,
            Err(e) => return Err(Error::SerializationError(e)),
        };
        Ok(json)
    }

    fn get_bytes(&self, url: &str) -> Result<Bytes, Error> {
        let response = self.get(url)?;
        let data = response.bytes();
        let data = match data {
            Ok(data) => data,
            Err(e) => return Err(Error::ReqwestError(e)),
        };
        Ok(data)
    }

    fn get(&self, url: &str) -> Result<reqwest::blocking::Response, Error> {
        let response = self.client
            .get(url)
            .send();
        let response = match response {
            Ok(resp) => resp,
            Err(e) => return Err(Error::ReqwestError(e)),
        };
        if response.status() == 429 {
            return Err(Error::TooManyRequests);
        }
        Ok(response)
    }

    fn post<T:Serialize>(&self, url: &str, json: T) -> Result<reqwest::blocking::Response, Error> {
        let json = serialize(json)?;
        let response = self.client
            .post(url)
            .json(&json)
            .send();
        let response = match response {
            Ok(resp) => resp,
            Err(e) => return Err(Error::ReqwestError(e)),
        };
        if response.status() == 429 {
            return Err(Error::TooManyRequests);
        }
        Ok(response)
    }
}