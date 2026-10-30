use dotenv::dotenv;
use tokio::sync::mpsc;
use tokio_postgres::{Client, NoTls};

async fn connect() -> Result<Client, Box<dyn std::error::Error + Send + Sync>> {
    dotenv().ok();
    let url = std::env::var("DATABASE_URL")?;
    let (client, connection) = tokio_postgres::connect(&url, NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("Ошибка соединения: {}", e);
        }
    });

    Ok(client)
}

async fn create_table(client: &Client) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS users (
                id SERIAL PRIMARY KEY,
                name VARCHAR NOT NULL,
                email VARCHAR NOT NULL
            )",
            &[],
        )
        .await?;
    Ok(())
}

async fn insert_user(
    client: &Client,
    name: &str,
    email: &str,
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let row = client
        .query_one(
            "INSERT INTO users (name, email) VALUES ($1, $2) RETURNING id",
            &[&name, &email],
        )
        .await?;
    Ok(row.get(0))
}

async fn list_users(
    client: &Client,
) -> Result<Vec<(i32, String, String)>, Box<dyn std::error::Error + Send + Sync>> {
    let rows = client
        .query("SELECT id, name, email FROM users", &[])
        .await?;
    let users: Vec<(i32, String, String)> = rows
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    Ok(users)
}

async fn insert_worker(
    mut rx: mpsc::Receiver<(String, String)>,
    client: Client,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    while let Some((name, email)) = rx.recv().await {
        match insert_user(&client, &name, &email).await {
            Ok(id) => println!("Worker: создан пользователь {} с id={}", name, id),
            Err(e) => eprintln!("Worker: ошибка вставки {}: {}", name, e),
        }
    }
    Ok(())
}

fn main() {
    let rt = tokio::runtime::Runtime::new().expect("Не удалось создать Tokio runtime");
    rt.block_on(async {
        let client = match connect().await {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Не удалось подключиться: {}", e);
                return;
            }
        };
        if let Err(e) = create_table(&client).await {
            eprintln!("Не удалось создать таблицу: {}", e);
            return;
        }
        match insert_user(&client, "Alice", "alice@example.com").await {
            Ok(id) => println!("Создан пользователь Alice с id={}", id),
            Err(e) => eprintln!("Ошибка вставки Alice: {}", e),
        }
        let (tx, rx) = mpsc::channel::<(String, String)>(100);
        let worker_handle = tokio::spawn(async move {
            if let Err(e) = insert_worker(rx, client).await {
                eprintln!("Worker завершился с ошибкой: {}", e);
            }
        });
        let users_to_insert = vec![
            ("Bob".to_string(), "bob@example.com".to_string()),
            ("Charlie".to_string(), "charlie@example.com".to_string()),
            ("Diana".to_string(), "diana@example.com".to_string()),
            ("Eve".to_string(), "eve@example.com".to_string()),
        ];
        for (name, email) in users_to_insert {
            if let Err(e) = tx.send((name, email)).await {
                eprintln!("Не удалось отправить в канал: {}", e);
                break;
            }
        }
        drop(tx);
        if let Err(e) = worker_handle.await {
            eprintln!("Worker упал: {}", e);
        }
        match list_users(&client).await {
            Ok(users) => {
                println!("\n=== Все пользователи ===");
                for u in users {
                    println!("id={}, name={}, email={}", u.0, u.1, u.2);
                }
            }
            Err(e) => eprintln!("Не удалось прочитать пользователей: {}", e),
        }
    });
}