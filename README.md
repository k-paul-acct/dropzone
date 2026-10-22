# DropZone 🚀

A minimal local-network file & message sharing CLI tool written in Rust.

<!-- 900x500 -->
![alt](images/home.png)

### Features

- 📁 **File uploads** --- connected devices can drag and drop or select files
- 💬 **Text messages** --- typed messages are printed directly to your terminal
- 🌐 **Zero config** --- just run the program and share the URL shown in the terminal
- 🎨 **Nice UI** --- dark-themed web page, works on mobile too

### Build & Run

```bash
# clone/copy the project, then:
cargo build --release

# run on the default port 8080
./target/release/dropzone serve

# or specify a custom port
./target/release/dropzone serve 5050
```

### Configuration

By default, DropZone runs with TLS (HTTPS mode). At each startup it generates a fresh self-signed ED25519 certificate and private key in memory.

> [!WARNING]
> Browsers will show a certificate warning. The generated certificate is not automatically trusted, and it changes every startup. Accepting the warning enables encrypted traffic but does not reliably verify the server's identity against an active attacker.

To use your own PEM certificate and private key instead, set both environment variables:

```bash
DROPZONE_CERT_PATH=/path/to/cert.pem DROPZONE_CERT_KEY_PATH=/path/to/key.pem ./target/release/dropzone serve
```

To run without TLS (unencrypted HTTP-only mode), use `dropzone serve --no-tls`. In this mode, certificate environment variables are ignored.

Use `dropzone serve --help` to show all server options.
