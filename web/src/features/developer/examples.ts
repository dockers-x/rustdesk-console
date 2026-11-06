export const environmentExample = `export RUSTDESK_CONSOLE_URL='https://console.example.com'
export RUSTDESK_CONSOLE_API_KEY='<API_KEY>'`;

export const sdkExamples = {
  ts: {
    label: "Node.js / TypeScript",
    repository: "https://github.com/lib-x/rustdesk-console-sdk-ts",
    install: "npm install github:lib-x/rustdesk-console-sdk-ts#v0.1.0",
    code: `import { Client } from "@lib-x/rustdesk-console-sdk";

const client = new Client({
  baseUrl: process.env.RUSTDESK_CONSOLE_URL!,
  apiKey: process.env.RUSTDESK_CONSOLE_API_KEY!,
});
const page = await client.listDevices({ page: 1, page_size: 10 });
for (const device of page.data) {
  console.log(device.id, device.hostname);
}`,
  },
  rust: {
    label: "Rust",
    repository: "https://github.com/lib-x/rustdesk-console-sdk-rs",
    install: "cargo add rustdesk-console-sdk\ncargo add tokio --features macros,rt-multi-thread",
    code: `use rustdesk_console_sdk::{Client, ListOptions};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("RUSTDESK_CONSOLE_URL")?;
    let key = std::env::var("RUSTDESK_CONSOLE_API_KEY")?;
    let client = Client::new(&url, &key)?;
    let page = client.list_devices(&ListOptions {
        page: Some(1),
        page_size: Some(10),
        ..Default::default()
    }).await?;
    for device in page.data {
        println!("{} {}", device.id, device.hostname);
    }
    Ok(())
}`,
  },
  go: {
    label: "Go",
    repository: "https://github.com/lib-x/rustdesk-console-sdk-go",
    install: "go get github.com/lib-x/rustdesk-console-sdk-go@v0.1.0",
    code: `package main

import (
    "context"
    "fmt"
    "log"
    "os"
    console "github.com/lib-x/rustdesk-console-sdk-go"
)

func main() {
    client, err := console.NewClient(
        os.Getenv("RUSTDESK_CONSOLE_URL"),
        os.Getenv("RUSTDESK_CONSOLE_API_KEY"),
    )
    if err != nil { log.Fatal(err) }
    page, err := client.ListDevices(context.Background(),
        &console.ListOptions{Page: 1, PageSize: 10})
    if err != nil { log.Fatal(err) }
    for _, device := range page.Data {
        fmt.Println(device.ID, device.Hostname)
    }
}`,
  },
};
