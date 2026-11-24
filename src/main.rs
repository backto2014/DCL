use mini_redis::{client, Result};

const URL: &str = "https://sober.vinegarhq.org/";


#[tokio::main]
async fn main() -> Result<()> {
    let mut client = client::connect("127.0.0.1:6379").await?;

    client.set("hello", "world".into()).await?;

    let result = client.get("hello").await?;

    //println!("server retornou; result={:?}", result);

    let html  = r#"<html><body><h1 class="titulo">Olá</h1><p>texto</p></body></html>"#;
    Ok(())
}