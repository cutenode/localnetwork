//! A plugin that does as little as it can, for trying the protocol: `cargo run --example echo`,
//! then type `{"jsonrpc":"2.0","id":1,"method":"hello","params":{"protocol":"0.2.0","host":{"name":"me","version":"0"}}}`.

mod plugin;

fn main() {
    loclib_plugin::serve(plugin::MANIFEST, plugin::hooks(), std::io::stdin(), std::io::stdout());
}
