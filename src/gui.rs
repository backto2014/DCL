use eframe::egui;


struct DCL {
    url: String,
    render: Vec<CRender>
}

enum CRender {
    Retangle { x: f32, y: f32, w: f32, h: f32, color: Color32 },
    Text { x: f32, y: f32, value: String, tam: f32, color: Color32 }
}

impl DCL {
    fn load(&mut self, url: &str) {

    }
}

impl eframe::App for DCL {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {

    }
}