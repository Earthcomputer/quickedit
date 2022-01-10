extern crate winres;

fn main() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_dir = std::path::Path::new(&out_dir);
    let gl_bindings_file = out_dir.join("gl_bindings.rs");
    gl_generator::Registry::new(gl_generator::Api::Gl, (4, 5), gl_generator::Profile::Compatibility, gl_generator::Fallbacks::None, Vec::new())
        .write_bindings(gl_generator::GlobalGenerator, &mut std::fs::File::create(gl_bindings_file).unwrap())
        .unwrap();

    if cfg!(target_os = "windows") {
        let mut res = winres::WindowsResource::new();
        res.set_icon("res/icon_windows.ico");
        res.compile().unwrap();
    }
}