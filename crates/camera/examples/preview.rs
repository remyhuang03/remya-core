use remya_camera::{Backend, native::Native, preview};
fn main() {
    for c in Native.read().unwrap() {
        let id = preview::start_preview(c.name, c.serial).unwrap();
        let result = preview::next_preview_frame(id);
        preview::stop_preview(id).unwrap();
        let frame = result.unwrap();
        assert_eq!(
            frame.rgba.len(),
            frame.width as usize * frame.height as usize * 4
        );
        println!("Preview {}: {}x{}", c.serial, frame.width, frame.height);
    }
}
