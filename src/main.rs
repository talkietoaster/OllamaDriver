use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::error::Error;
use sha2::{Sha256, Digest};
use redis::{Commands, RedisResult};
use std::collections::HashMap;

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
        r#"Explain what this Rust function does:
fn add(a: i32, b: i32) -> i32 {
    a + b
}"#,
        r#"Explain the concept of ownership in Rust."#,
        // Add more prompts as needed
    ];

    for prompt in prompts {
        make_api_call(&client, &prompt).await?;
    }

    Ok(())
}

async fn store_and_fetch_qa(key: &str, question: &str, answer: &str) -> RedisResult<()> {
    // Connect to local Redis server
    let client = redis::Client::open("redis://127.0.0.1/")?;
    let mut con = client.get_connection()?;

    // Store question and answer in a hash using the computed key
    let _: () = con.hset_multiple(key, &[("question", question), ("answer", answer)])?;

    // Retrieve the full hash
    let q_and_a: HashMap<String, String> = con.hgetall(key)?;

    // Print the Q&A
    println!("Question: {}", q_and_a.get("question").unwrap_or(&"<missing>".to_string()));
    println!("Answer: {}", q_and_a.get("answer").unwrap_or(&"<missing>".to_string()));

    Ok(())
}

async fn make_api_call(client: &Client, prompt: &str) -> Result<String, Box<dyn Error>> {
    // Generate SHA-256 hash of the prompt
    let prompt_hash = sha256_digest(prompt);
    println!("Prompt Hash: {}", prompt_hash);

    let request_body = OllamaRequest {
        model: "llama2".to_string(), // replace with your model name
        prompt: prompt.to_string(),
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
    println!("Explanation:");
    let mut answer = String::new();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        let text = std::str::from_utf8(&chunk)?;
        for line in text.lines() {
            if let Ok(parsed) = serde_json::from_str::<OllamaStreamChunk>(line) {
                answer.push_str(&parsed.response);
                print!("{}", parsed.response);
            }
        }
    }

    store_and_fetch_qa(&prompt_hash, prompt, &answer).await?;
    Ok(answer)
}