fn main() {
    #[cfg(windows)]
    embed_resource::compile("mapflow.rc", embed_resource::NONE);
}
