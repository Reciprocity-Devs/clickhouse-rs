use std::time::Duration;

use serde::{Deserialize, Serialize};

use typed_clickhouse::{error::Result, Client, Reflection};

#[derive(Debug, Reflection, Serialize, Deserialize)]
struct Row<'a> {
    no: u32,
    name: &'a str,
}

async fn ddl(client: &Client) -> Result<()> {
    client.query("DROP TABLE IF EXISTS some").execute().await?;
    client
        .query(
            "
            CREATE TABLE some(no UInt32, name LowCardinality(String))
            ENGINE = MergeTree
            ORDER BY no
        ",
        )
        .execute()
        .await
}

async fn insert(client: &Client) -> Result<()> {
    let mut insert = client.insert("some")?;
    for i in 0..1000 {
        insert.write(&Row { no: i, name: "foo" }).await?;
    }

    insert.end().await
}

async fn inserter(client: &Client) -> Result<()> {
    let mut inserter = client
        .inserter("some")?
        .with_entry_threshold(100_000)
        .with_time_threshold(Duration::from_secs(15));

    for i in 0..1000 {
        if i == 500 {
            inserter.set_entry_threshold(300);
        }

        inserter.write(&Row { no: i, name: "foo" }).await?;
        inserter.commit().await?;
    }

    inserter.end().await?;
    Ok(())
}

async fn select(client: &Client) -> Result<()> {
    let mut cursor = client
        .query("SELECT ?fields FROM some WHERE no BETWEEN ? AND ?")
        .bind(500)
        .bind(504)
        .rows::<Row<'_>>()?;

    while let Some(row) = cursor.next().await? {
        println!("{:?}", row);
    }

    Ok(())
}

async fn delete(client: &Client) -> Result<()> {
    use tokio::time;

    client
        .query("ALTER TABLE some DELETE WHERE no >= ?")
        .bind(500)
        .execute()
        .await?;

    // Mutations are async, so we are waiting for some time.
    time::delay_for(time::Duration::from_millis(100)).await;
    Ok(())
}

async fn select_count(client: &Client) -> Result<()> {
    let mut cursor = client.query("SELECT count() FROM some").rows::<u64>()?;
    let count = cursor.next().await?.unwrap();
    println!("count() = {}", count);

    Ok(())
}

async fn watch(client: &Client) -> Result<()> {
    let mut cursor = client
        .watch("SELECT max(no), argMax(name, no) FROM some")
        .rows::<Row<'_>>()?;

    let (version, row) = cursor.next().await?.unwrap();
    println!("version={}, row={:?}", version, row);

    let mut insert = client.insert("some")?;
    let row = Row {
        no: row.no + 1,
        name: "bar",
    };
    insert.write(&row).await?;
    insert.end().await?;

    let (version, row) = cursor.next().await?.unwrap();
    println!("version={}, row={:?}", version, row);

    // Or you can request only events without data.
    let mut cursor = client
        // It's possible to specify a view name.
        .watch("lv_e1d9c5dc4a1e7f5f2fb41398156d41c88ef9ab74")
        .limit(10)
        .events()?;

    println!("{:?}", cursor.next().await);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::default().with_url("http://localhost:8123");

    ddl(&client).await?;
    insert(&client).await?;
    inserter(&client).await?;
    select_count(&client).await?;
    select(&client).await?;
    delete(&client).await?;
    select_count(&client).await?;
    watch(&client).await?;

    Ok(())
}
