use tauri::image::Image;
fn test() {
    let rgba = vec![0u8; 16];
    let img = Image::new(&rgba, 2, 2);
}
