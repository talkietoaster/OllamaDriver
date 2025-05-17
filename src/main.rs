use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use sha2::{Sha256, Digest};
use redis::{Commands, RedisResult};
use std::collections::HashMap;
use flate2::{Compression, write::ZlibEncoder};
use base64::{engine::general_purpose, Engine as _};
use std::io::Write;
use flate2::read::ZlibDecoder;
use std::io::Read;


const REDIS_ADDRESS: &str = "redis://127.0.0.1/";
fn compress_str(input: &str) -> String {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();
    general_purpose::STANDARD.encode(compressed) // Store as base64 string
}

fn decompress_str(encoded: &str) -> String {
    let compressed = general_purpose::STANDARD.decode(encoded).unwrap();
    let mut decoder = ZlibDecoder::new(&compressed[..]);
    let mut decompressed = String::new();
    decoder.read_to_string(&mut decompressed).unwrap();
    decompressed
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    prompt: String,
    stream: bool,
}

#[derive(Deserialize)]
struct OllamaStreamChunk {
    response: String,
}

// Function to generate SHA-256 hash of the prompt
fn sha256_digest(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    format!("{:x}", hasher.finalize())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = Client::new();

    // List of prompts to process
    let prompts = vec![
        r#"Explain what this Rust  function does: fn add(a: i32, b: i32) -> i32 { a + b }"#,
        r#"   Explain  the concept of ownership  in  Rust.                                     "#,
        // Add more prompts as needed
    ];

    for prompt in prompts {
        make_api_call(&client, &prompt).await?;
    }

    Ok(())
}

async fn store_and_fetch_qa(key: &str, question: &str, answer: &str) -> RedisResult<()> {
    // Connect to local Redis server
    let client = redis::Client::open(REDIS_ADDRESS)?;
    let mut con = client.get_connection()?;

    // Trim the question to remove any leading or trailing whitespace
    let trimmed_question = question.trim();

    let compressed_answer = compress_str(answer);
    
    println!("Storing compressed answer in Redis...{}", compressed_answer);
    // Store question and answer in a hash using the computed key
    let _: () = con.hset_multiple(&key, &[("question", trimmed_question), ("answer", &compressed_answer)])?;
    Ok(())
}

async fn fetch_qa_from_db(key: &str) -> RedisResult<Option<(String, String)>> {
    // Connect to local Redis server
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_connection()?;

    // Retrieve the full hash
    let q_and_a: HashMap<String, String> = con.hgetall(key)?;

    if q_and_a.is_empty() {
        return Ok(None);
    }

    let question = q_and_a.get("question").cloned().unwrap_or_default();
    let answer = q_and_a.get("answer").cloned().unwrap_or_default();
    let answer = decompress_str(&answer);

    Ok(Some((question, answer)))
}

async fn make_api_call(client: &Client, prompt: &str) -> Result<String, Box<dyn Error>> {
    // Trim the question to remove any leading or trailing whitespace
    let trimmed_prompt = prompt.trim();

    // Generate SHA-256 hash of the trimmed prompt
    let prompt_hash = sha256_digest(trimmed_prompt);
    println!("Prompt Hash: {}", prompt_hash);

    // Check if the answer already exists in Redis
    match fetch_qa_from_db(&prompt_hash).await? {
        Some((_, answer)) => {
            println!("HIT!");
            println!("{}", "🚀".repeat(10));  // Print a line of emoji rocket ships
            print!("{}", answer);
            return Ok(answer);
        }
        None => {}
    }

    let request_body = OllamaRequest {
        model: "llama2".to_string(), // replace with your model name
        prompt: trimmed_prompt.to_string(),
        stream: true,
    };

    let response = client
        .post("http://localhost:11434/api/generate")
        .json(&request_body)
        .send()
        .await?;

    if !response.status().is_success() {
        eprintln!("Failed to get explanation: {}", response.status());
        return Ok(String::new()); // Return an empty string on failure
    }

    let stream = response.bytes_stream();
    tokio::pin!(stream);
    use futures_util::StreamExt;

    println!("Generating Explanation...");
    let mut answer = String::new();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        let text = std::str::from_utf8(&chunk)?;
        for line in text.lines() {
            if let Ok(parsed) = serde_json::from_str::<OllamaStreamChunk>(line) {
                answer.push_str(&parsed.response);
            }
        }
    }

    println!("{}", "😂".repeat(10));  // Print a line of emoji laughing faces
    print!("{}", answer);

    store_and_fetch_qa(&prompt_hash, trimmed_prompt, &answer).await?;

    Ok(answer)
}