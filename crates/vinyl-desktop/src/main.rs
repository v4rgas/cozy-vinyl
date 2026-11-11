mod audio;
mod commons;
mod environment;
mod mesh;
mod records;
mod room;
mod scene;

use audio::{AudioController, Command, Snapshot};
use eframe::{egui, egui_glow, glow};
use records::LoadedRecord;
use scene::{RecordVisual, Scene, SceneState};
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
    time::{Duration, Instant},
};
use vinyl_core::{geometry, Condition, Parameters, Record, Stylus};

#[derive(Clone)]
enum Source {
    Builtin(String),
    Files(Vec<PathBuf>),
    Commons(commons::Track),
}
struct AlbumSession {
    tracks: Vec<commons::Track>,
    index: usize,
    generation: u64,
    next: Option<Result<LoadedRecord, String>>,
}
enum AlbumEvent {
    Listed(u64, Result<Vec<commons::Track>, String>),
    Prepared(u64, usize, Result<LoadedRecord, String>),
}
struct Pending {
    generation: u64,
    result: Result<LoadedRecord, String>,
}
enum Drag {
    Arm(f64),
    Platter { angle: f32 },
    Orbit,
}
struct VinylApp {
    scene: Arc<Mutex<Scene>>,
    view: SceneState,
    audio: Option<AudioController>,
    snapshot: Option<Snapshot>,
    root: PathBuf,
    source: Source,
    record: Option<Arc<Record>>,
    title: String,
    credit: String,
    status: String,
    busy: bool,
    generation: u64,
    tx: mpsc::Sender<Pending>,
    rx: mpsc::Receiver<Pending>,
    parameters: Parameters,
    stylus: Stylus,
    tracking: f64,
    antiskate: f64,
    capacitance: f64,
    condition: Condition,
    seed: u32,
    speed: f64,
    pitch: f64,
    motor: bool,
    cue: bool,
    controls: bool,
    shelf: bool,
    hud: bool,
    drag: Option<Drag>,
    last_frame: Instant,
    album: Option<AlbumSession>,
    album_tx: mpsc::Sender<AlbumEvent>,
    album_rx: mpsc::Receiver<AlbumEvent>,
}
impl VinylApp {
    fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> Result<Self, String> {
        let gl = cc.gl.as_ref().ok_or("OpenGL is unavailable")?;
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(15, 15, 18);
        visuals.window_fill = egui::Color32::from_rgba_unmultiplied(24, 24, 28, 245);
        visuals.selection.bg_fill = egui::Color32::from_rgb(115, 71, 29);
        cc.egui_ctx.set_visuals(visuals);
        let (tx, rx) = mpsc::channel();
        let (album_tx, album_rx) = mpsc::channel();
        let mut app = Self {
            scene: Arc::new(Mutex::new(Scene::new(gl, &root)?)),
            view: SceneState::default(),
            audio: None,
            snapshot: None,
            root,
            source: Source::Builtin("rhapsody".into()),
            record: None,
            title: "Cozy Vinyl".into(),
            credit: String::new(),
            status: String::new(),
            busy: false,
            generation: 0,
            tx,
            rx,
            parameters: Parameters::default(),
            stylus: Stylus::Elliptical,
            tracking: 1.8,
            antiskate: 1.8,
            capacitance: 200.0,
            condition: Condition::default(),
            seed: 7,
            speed: 100.0 / 3.0,
            pitch: 0.0,
            album: None,
            album_tx,
            album_rx,
            motor: false,
            cue: true,
            controls: false,
            shelf: false,
            hud: false,
            drag: None,
            last_frame: Instant::now(),
        };
        app.load(app.source.clone());
        Ok(app)
    }
    fn send(&mut self, c: Command) {
        if let Some(audio) = &mut self.audio {
            if !audio.command(c) {
                self.status = "Audio controls are busy. Try again.".into();
            }
        }
    }
    fn power(&mut self) {
        if self.audio.is_none() {
            match AudioController::start() {
                Ok(audio) => self.audio = Some(audio),
                Err(e) => {
                    self.status = e;
                    return;
                }
            }
            self.push_parameters();
            if let Some(record) = self.record.clone() {
                self.send(Command::Record {
                    record,
                    sequence: self.generation,
                });
            }
        }
        self.motor = true;
        self.send(Command::Motor(true));
        self.send(Command::Speed(self.speed * (1.0 + self.pitch / 100.0)));
        if self.record.is_some() {
            self.send(Command::CueTo(geometry::alpha_for_radius(
                geometry::R_LEADIN - 0.0012,
            )));
        }
    }
    fn push_parameters(&mut self) {
        self.send(Command::Parameters(self.parameters));
        self.send(Command::Stylus(self.stylus));
        self.send(Command::TrackingForce(self.tracking));
        self.send(Command::Antiskate(self.antiskate));
        self.send(Command::Capacitance(self.capacitance));
    }
    fn toggle_motor(&mut self) {
        if self.audio.is_none() {
            self.power();
            return;
        }
        self.motor = !self.motor;
        self.send(Command::Motor(self.motor));
    }
    fn toggle_cue(&mut self) {
        self.cue = !self.cue;
        self.send(Command::Cue(self.cue));
    }
    fn load(&mut self, source: Source) {
        if !matches!(source, Source::Commons(_)) {
            self.album = None;
        }
        self.source = source.clone();
        self.generation += 1;
        self.busy = true;
        self.status = "Preparing the groove…".into();
        self.send(Command::Cue(true));
        self.send(Command::Motor(false));
        let root = self.root.clone();
        let condition = self.condition;
        let seed = self.seed;
        let tx = self.tx.clone();
        let generation = self.generation;
        std::thread::spawn(move || {
            let result = match source {
                Source::Builtin(id) => records::load_builtin(&root, &id, Some(condition), seed),
                Source::Files(paths) => records::load_files(&paths, condition, seed),
                Source::Commons(track) => {
                    commons::prepare_track(&track, &root.join("cache"), condition, seed)
                }
            };
            let _ = tx.send(Pending { generation, result });
        });
    }
    fn choose_album(&mut self, album: commons::Album) {
        self.generation += 1;
        self.album = None;
        self.busy = true;
        self.status = "Loading album…".into();
        self.send(Command::Cue(true));
        self.send(Command::Motor(false));
        let tx = self.album_tx.clone();
        let cache = self.root.join("cache");
        let generation = self.generation;
        std::thread::spawn(move || {
            let _ = tx.send(AlbumEvent::Listed(
                generation,
                commons::load_album(&album, &cache),
            ));
        });
    }
    fn prefetch(&mut self) {
        let Some(album) = &self.album else {
            return;
        };
        let index = album.index + 1;
        let Some(track) = album.tracks.get(index).cloned() else {
            return;
        };
        let generation = album.generation;
        let tx = self.album_tx.clone();
        let cache = self.root.join("cache");
        let condition = self.condition;
        let seed = self.seed;
        std::thread::spawn(move || {
            let result = commons::prepare_track(&track, &cache, condition, seed);
            let _ = tx.send(AlbumEvent::Prepared(generation, index, result));
        });
    }
    fn choose_track(&mut self, index: usize) {
        let Some(track) = self
            .album
            .as_ref()
            .and_then(|a| a.tracks.get(index))
            .cloned()
        else {
            return;
        };
        if let Some(a) = &mut self.album {
            a.index = index;
            a.next = None;
            a.generation += 1;
        }
        self.load(Source::Commons(track));
    }
    fn import(&mut self) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter("Audio", &["flac", "mp3", "wav", "ogg", "m4a", "aac"])
            .pick_files()
        {
            self.load(Source::Files(paths));
        }
    }
    fn accept(&mut self, loaded: LoadedRecord, gl: &glow::Context) {
        let rec = &loaded.record;
        let mut warp = [[0.0; 3]; 3];
        for (dst, src) in warp.iter_mut().zip(&rec.warp) {
            *dst = [src.k as f32, src.amp as f32, src.ph as f32];
        }
        let visual = RecordVisual {
            r_table: rec.r_tab.iter().map(|x| *x as f32).collect(),
            theta_end: rec.theta_end,
            modulation: loaded.visual.data,
            cols: loaded.visual.cols,
            rows: loaded.visual.rows,
            warp,
            scratches: rec
                .scratches
                .iter()
                .map(|s| [s.r0 as f32, s.r1 as f32, s.phi0 as f32, s.phi1 as f32])
                .collect(),
            dust: rec.condition.dust as f32,
            ecc: [rec.ecc[0] as f32, rec.ecc[1] as f32],
            titles: rec.tracks.iter().map(|t| t.title.clone()).collect(),
        };
        self.scene.lock().unwrap().set_record(gl, &visual);
        self.status = if rec.truncated {
            "The programme was too long for one side and was cut short.".into()
        } else {
            String::new()
        };
        self.title = loaded.title;
        self.credit = loaded.credit;
        self.record = Some(loaded.record.clone());
        self.view.record = true;
        self.send(Command::Record {
            record: loaded.record,
            sequence: self.generation,
        });
        self.cue = true;
        if self.album.is_some() {
            self.prefetch();
        }
        if self.motor {
            self.send(Command::Motor(true));
            self.send(Command::CueTo(geometry::alpha_for_radius(
                geometry::R_LEADIN - 0.0012,
            )));
        }
    }
    fn controls(&mut self, ctx: &egui::Context) {
        let mut open = self.controls;
        egui::Window::new("Controls")
            .open(&mut open)
            .default_pos([890.0, 70.0])
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(if self.motor { "Stop" } else { "Start" })
                        .clicked()
                    {
                        self.toggle_motor();
                    }
                    if ui
                        .button(if self.cue { "Lower cue" } else { "Lift cue" })
                        .clicked()
                    {
                        self.toggle_cue();
                    }
                    if ui.button("Return arm").clicked() {
                        self.send(Command::CueTo(geometry::alpha_rest()));
                    }
                });
                ui.horizontal(|ui| {
                    for (rpm, label) in [(100.0 / 3.0, "33⅓"), (45.0, "45"), (78.26, "78")] {
                        if ui
                            .selectable_label((self.speed - rpm).abs() < 0.1, label)
                            .clicked()
                        {
                            self.speed = rpm;
                            self.send(Command::Speed(rpm * (1.0 + self.pitch / 100.0)));
                        }
                    }
                });
                if ui
                    .add(egui::Slider::new(&mut self.pitch, -8.0..=8.0).text("Pitch %"))
                    .changed()
                {
                    self.send(Command::Speed(self.speed * (1.0 + self.pitch / 100.0)));
                }
                let mut changed = ui
                    .add(egui::Slider::new(&mut self.parameters.volume, 0.0..=1.0).text("Volume"))
                    .changed();
                ui.separator();
                ui.label("Cartridge");
                egui::ComboBox::from_id_salt("stylus")
                    .selected_text(format!("{:?}", self.stylus))
                    .show_ui(ui, |ui| {
                        for (s, label) in [
                            (Stylus::Elliptical, "Elliptical 0.3 × 0.7 mil"),
                            (Stylus::Conical, "Conical 0.7 mil"),
                            (Stylus::Microline, "Micro-line"),
                        ] {
                            changed |= ui.selectable_value(&mut self.stylus, s, label).changed();
                        }
                    });
                changed |= ui
                    .add(egui::Slider::new(&mut self.tracking, 0.2..=4.0).text("Tracking g"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut self.antiskate, 0.0..=4.0).text("Anti-skate"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut self.parameters.hum, 0.0..=3.0).text("Mains hum"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut self.parameters.rumble, 0.0..=3.0).text("Rumble"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut self.parameters.flutter, 0.0..=3.0).text("Flutter"))
                    .changed();
                changed |= ui
                    .add(egui::Slider::new(&mut self.capacitance, 50.0..=600.0).text("Load pF"))
                    .changed();
                if changed {
                    self.push_parameters();
                }
                ui.separator();
                ui.label("This pressing");
                ui.add(egui::Slider::new(&mut self.condition.noise, 0.0..=4.0).text("Surface"));
                ui.add(egui::Slider::new(&mut self.condition.dust, 0.0..=3.0).text("Dust"));
                ui.add(
                    egui::Slider::new(&mut self.condition.scratches, 0.0..=2.0).text("Scratches"),
                );
                ui.add(egui::Slider::new(&mut self.condition.warp, 0.0..=2.0).text("Warp mm"));
                ui.add(
                    egui::Slider::new(&mut self.condition.off_center, 0.0..=1.5)
                        .text("Off-centre mm"),
                );
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.busy, egui::Button::new("Press again"))
                        .clicked()
                    {
                        self.seed = self.seed.wrapping_add(7919);
                        self.load(self.source.clone());
                    }
                    if ui.button("Cut a file").clicked() {
                        self.import();
                    }
                });
                if ui.button("Knock the table").clicked() {
                    self.send(Command::Knock(1.0));
                }
                ui.checkbox(&mut self.hud, "Show physics");
            });
        self.controls = open;
    }
    fn shelf(&mut self, ctx: &egui::Context) {
        let mut open = self.shelf;
        egui::Window::new("Records")
            .open(&mut open)
            .default_pos([20.0, 70.0])
            .default_width(290.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(550.0)
                    .show(ui, |ui| {
                        for entry in records::catalog() {
                            if ui
                                .button(format!("{}\n{}", entry.title, entry.by))
                                .clicked()
                            {
                                self.load(Source::Builtin(entry.id.to_owned()));
                            }
                            ui.horizontal(|ui| {
                                ui.small(entry.licence);
                                if !entry.url.is_empty() {
                                    ui.hyperlink_to("Source", entry.url);
                                }
                            });
                        }
                        ui.separator();
                        ui.label("CC0 albums");
                        for album in commons::catalog() {
                            if ui
                                .button(format!("{}\n{}", album.title, album.by))
                                .clicked()
                            {
                                self.choose_album(*album);
                            }
                            ui.hyperlink_to("Source", commons::album_source(album));
                        }
                        let tracks = self
                            .album
                            .as_ref()
                            .map(|a| {
                                a.tracks
                                    .iter()
                                    .map(|t| (t.number, t.title.clone()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        for (i, title) in tracks {
                            if ui.button(format!("{i}. {title}")).clicked() {
                                self.choose_track(i - 1);
                            }
                        }
                        ui.separator();
                        if ui.button("Cut audio files…").clicked() {
                            self.import();
                        }
                    });
            });
        self.shelf = open;
    }
    fn pointer(&mut self, ctx: &egui::Context, response: &egui::Response, rect: egui::Rect) {
        let point = ctx.input(|i| i.pointer.interact_pos()).map(|p| {
            [
                2.0 * (p.x - rect.left()) / rect.width() - 1.0,
                1.0 - 2.0 * (p.y - rect.top()) / rect.height(),
            ]
        });
        let world = point.and_then(|p| self.scene.lock().unwrap().pick(p, 0.0));
        let arm_world = point.and_then(|p| self.scene.lock().unwrap().pick(p, 0.018));
        if response.drag_started() {
            let plain = ctx
                .input(|i| !i.modifiers.shift && !i.modifiers.alt && !i.pointer.secondary_down());
            self.drag = Some(
                if plain
                    && self.audio.is_some()
                    && arm_world
                        .is_some_and(|p| Scene::arm_distance(p, self.view.arm_angle) < 0.012)
                {
                    let p = arm_world.unwrap();
                    let angle = arm_angle(p);
                    self.send(Command::Hand(true, self.view.arm_angle as f64));
                    Drag::Arm(self.view.arm_angle as f64 - angle)
                } else if plain
                    && self.audio.is_some()
                    && world.is_some_and(|p| p[0].hypot(p[1]) < geometry::R_DISC as f32)
                {
                    let p = world.unwrap();
                    self.send(Command::PlatterHand(true));
                    Drag::Platter {
                        angle: p[1].atan2(p[0]),
                    }
                } else {
                    Drag::Orbit
                },
            );
        }
        if response.dragged() {
            match self.drag.take() {
                Some(Drag::Arm(offset)) => {
                    if let Some(p) = arm_world {
                        self.send(Command::Hand(true, arm_angle(p) + offset));
                    }
                    self.drag = Some(Drag::Arm(offset));
                }
                Some(Drag::Platter { angle }) => {
                    let mut next = angle;
                    if let Some(p) = world {
                        next = p[1].atan2(p[0]);
                        let d = (next - angle + std::f32::consts::PI)
                            .rem_euclid(std::f32::consts::TAU)
                            - std::f32::consts::PI;
                        self.send(Command::PlatterMove(-d as f64));
                    }
                    self.drag = Some(Drag::Platter { angle: next });
                }
                Some(Drag::Orbit) => {
                    let delta = ctx.input(|i| i.pointer.delta());
                    self.view.azimuth = (self.view.azimuth - delta.x * 0.006).clamp(-1.15, 1.15);
                    self.view.elevation = (self.view.elevation + delta.y * 0.006).clamp(0.15, 1.1);
                    self.drag = Some(Drag::Orbit);
                }
                None => {}
            }
        }
        if response.drag_stopped() || ctx.input(|i| !i.focused) {
            match self.drag.take() {
                Some(Drag::Arm(_)) => self.send(Command::Hand(false, self.view.arm_angle as f64)),
                Some(Drag::Platter { .. }) => self.send(Command::PlatterHand(false)),
                _ => {}
            }
        }
        if response.clicked() {
            if self.audio.is_none() {
                self.power();
            } else if let Some(p) = world {
                let r = p[0].hypot(p[1]);
                if r > 0.052 && r < geometry::R_DISC as f32 {
                    self.send(Command::CueTo(geometry::alpha_for_radius(r as f64)));
                } else if p[0] > -0.139 && p[0] < -0.097 && p[1] > -0.184 && p[1] < -0.158 {
                    self.toggle_motor();
                } else if p[0] > -0.088 && p[0] < -0.038 && p[1] > -0.19 && p[1] < -0.17 {
                    self.speed = if p[0] < -0.065 { 100.0 / 3.0 } else { 45.0 };
                    self.send(Command::Speed(self.speed * (1.0 + self.pitch / 100.0)));
                } else if p[0] > 0.225 && p[0] < 0.265 && p[1] > -0.13 && p[1] < -0.09 {
                    self.pitch = 0.0;
                    self.send(Command::Speed(self.speed));
                }
            }
        }
        if response.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            self.view.zoom = (self.view.zoom + scroll * 0.0015).clamp(-0.3, 1.6);
        }
    }
}
fn arm_angle(p: [f32; 2]) -> f64 {
    ((p[1] as f64 - geometry::pivot_y()).atan2(p[0] as f64 - geometry::pivot_x()))
        .rem_euclid(std::f64::consts::TAU)
}
impl eframe::App for VinylApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(pending) = self.rx.try_recv() {
            if pending.generation == self.generation {
                self.busy = false;
                match pending.result {
                    Ok(loaded) => {
                        if let Some(gl) = frame.gl() {
                            self.accept(loaded, gl);
                        }
                    }
                    Err(e) => self.status = e,
                }
            }
        }
        while let Ok(event) = self.album_rx.try_recv() {
            match event {
                AlbumEvent::Listed(generation, result) if generation == self.generation => {
                    match result {
                        Ok(tracks) => {
                            self.album = Some(AlbumSession {
                                tracks,
                                index: 0,
                                generation,
                                next: None,
                            });
                            self.choose_track(0);
                        }
                        Err(e) => {
                            self.busy = false;
                            self.status = e;
                        }
                    }
                }
                AlbumEvent::Prepared(generation, index, result) => {
                    if let Some(a) = &mut self.album {
                        if a.generation == generation && a.index + 1 == index {
                            a.next = Some(result);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some(audio) = &mut self.audio {
            if let Some(snapshot) = audio.latest_snapshot() {
                self.snapshot = Some(snapshot);
            }
            if let Some(e) = audio.take_error() {
                self.status = e;
            }
        }
        if let Some(s) = self.snapshot {
            let t = s.telemetry;
            self.view.platter_angle = t.thp as f32;
            self.view.arm_angle = t.al as f32;
            self.view.stylus_height = (t.zb + t.ze) as f32;
            self.cue = t.cue_up;
        }
        let ended = self.motor
            && !self.busy
            && self.snapshot.is_some_and(|s| {
                s.sequence == self.generation
                    && !s.telemetry.cue_up
                    && s.telemetry.hc < 40e-6
                    && s.telemetry.wp > 0.0
                    && self
                        .record
                        .as_ref()
                        .and_then(|r| r.tracks.last())
                        .is_some_and(|t| s.telemetry.theta >= t.theta1)
            });
        if ended && self.album.is_some() {
            let finished = self
                .album
                .as_ref()
                .is_some_and(|a| a.index + 1 >= a.tracks.len());
            if finished {
                self.motor = false;
                self.send(Command::Cue(true));
                self.send(Command::Motor(false));
                self.status = "Album finished. Select a track to play again.".into();
            } else if let Some(next) = self.album.as_mut().and_then(|a| a.next.take()) {
                match next {
                    Ok(loaded) => {
                        self.generation += 1;
                        if let Some(a) = &mut self.album {
                            a.index += 1;
                            self.source = Source::Commons(a.tracks[a.index].clone());
                        }
                        self.send(Command::Cue(true));
                        if let Some(gl) = frame.gl() {
                            self.accept(loaded, gl);
                        }
                    }
                    Err(e) => {
                        self.motor = false;
                        self.send(Command::Motor(false));
                        self.status = e;
                    }
                }
            } else {
                self.status = "Preparing the next track…".into();
            }
        }
        self.view.motor = self.motor;
        self.view.rpm = self.speed as f32;
        if !ctx.wants_keyboard_input() {
            ctx.input(|i| {
                if i.key_pressed(egui::Key::Space) {
                    self.toggle_motor();
                }
                if i.key_pressed(egui::Key::C) {
                    self.toggle_cue();
                }
                if i.key_pressed(egui::Key::B) {
                    self.send(Command::Knock(1.0));
                }
                if i.key_pressed(egui::Key::Z) {
                    self.view.inspect_stylus = !self.view.inspect_stylus;
                    self.view.zoom = 0.0;
                }
            });
        }
        let paths: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if !paths.is_empty() {
            self.load(Source::Files(paths));
        }
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("cozy vinyl").size(22.0));
                ui.separator();
                if ui.selectable_label(self.shelf, &self.title).clicked() {
                    self.shelf = !self.shelf;
                }
                if ui.selectable_label(self.controls, "Controls").clicked() {
                    self.controls = !self.controls;
                }
                if ui.button("Reset view").clicked() {
                    let record = self.view.record;
                    self.view = SceneState {
                        record,
                        ..Default::default()
                    };
                }
                if self.busy {
                    ui.spinner();
                }
            });
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("bottom").show(ctx, |ui| {
            if !self.status.is_empty() {
                ui.label(&self.status);
            } else {
                ui.small(&self.credit);
            }
            ui.horizontal(|ui| {
                ui.small("Drag the record to scratch. Drag the arm to cue. Scroll to zoom.");
                if self.audio.is_none() && ui.button("Power on").clicked() {
                    self.power();
                }
            });
        });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
                self.pointer(ctx, &response, rect);
                let scene = self.scene.clone();
                let state = self.view.clone();
                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                        let v = info.viewport_in_pixels();
                        scene.lock().unwrap().paint(
                            painter.gl(),
                            [v.left_px, v.from_bottom_px, v.width_px, v.height_px],
                            &state,
                        );
                    })),
                });
            });
        if self.controls {
            self.controls(ctx);
        }
        if self.shelf {
            self.shelf(ctx);
        }
        if self.hud {
            egui::Window::new("Physics")
                .default_pos([20.0, 420.0])
                .show(ctx, |ui| {
                    if let Some(s) = self.snapshot {
                        let t = s.telemetry;
                        if let Some(record) = &self.record {
                            if s.sequence == self.generation {
                                draw_groove(ui, record, &t);
                            }
                        }
                        egui::Grid::new("telemetry").show(ui, |ui| {
                            for (name, value) in [
                                (
                                    "Speed",
                                    format!("{:.2} rpm", t.wp * 60.0 / std::f64::consts::TAU),
                                ),
                                ("Groove", format!("{:.3} m/s", t.vg)),
                                ("Radius", format!("{:.2} mm", t.r * 1000.0)),
                                ("Tracking error", format!("{:.2}°", t.err.to_degrees())),
                                ("Left wall", format!("{:.2} mN", t.n_in * 1000.0)),
                                ("Right wall", format!("{:.2} mN", t.n_out * 1000.0)),
                                ("Mistracking", format!("{:.1}%", t.loss * 100.0)),
                                ("Cantilever", format!("{:.2} µm", t.xi * 1e6)),
                                ("DSP load", format!("{:.1}%", s.load * 100.0)),
                                ("Output peak", format!("{:.3}", s.peak)),
                            ] {
                                ui.label(name);
                                ui.monospace(value);
                                ui.end_row();
                            }
                        });
                    } else {
                        ui.label("Power on to inspect the physics.");
                    }
                    if let Some(a) = &self.audio {
                        ui.small(format!(
                            "{} — {} Hz, {} channels, {}",
                            a.info().name,
                            a.info().sample_rate,
                            a.info().channels,
                            a.info().format
                        ));
                    }
                });
        }
        self.last_frame = Instant::now();
        ctx.request_repaint_after(Duration::from_millis(16));
    }
    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        if let Some(gl) = gl {
            self.scene.lock().unwrap().destroy(gl);
        }
    }
}
// The microscope displays the same stored wall geometry that the stylus reads.
fn draw_groove(ui: &mut egui::Ui, record: &Record, t: &vinyl_core::Telemetry) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(310.0, 95.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, egui::Color32::from_rgb(28, 28, 32));
    let base = geometry::groove_radius(&record.r_tab, t.theta);
    let s0 = t.theta * record.spr / std::f64::consts::TAU;
    for groove in -2..=2 {
        for wall in 0..2 {
            let mut points = Vec::with_capacity(100);
            for column in 0..100 {
                let ds = (column as f64 / 99.0 * 2.0 - 1.0) * 160.0;
                let theta = t.theta
                    + groove as f64 * std::f64::consts::TAU
                    + ds * std::f64::consts::TAU / record.spr;
                let index = (s0 + groove as f64 * record.spr + ds).round() as isize;
                if theta < 0.0
                    || theta > record.theta_end
                    || index < 0
                    || index >= record.n as isize
                {
                    continue;
                }
                let center = geometry::groove_radius(&record.r_tab, theta) - base;
                let offset = if wall == 0 {
                    -record.w + std::f64::consts::SQRT_2 * record.wall_in[index as usize] as f64
                } else {
                    record.w - std::f64::consts::SQRT_2 * record.wall_out[index as usize] as f64
                };
                points.push(egui::pos2(
                    rect.left() + column as f32 / 99.0 * rect.width(),
                    rect.center().y - ((center + offset) / 95e-6) as f32 * rect.height() / 2.0,
                ));
            }
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(185, 178, 166)),
            ));
        }
    }
    painter.circle_filled(
        egui::pos2(
            rect.center().x,
            rect.center().y - (t.xg / 95e-6) as f32 * rect.height() / 2.0,
        ),
        5.0,
        if t.hc < 40e-6 {
            egui::Color32::from_rgb(255, 180, 84)
        } else {
            egui::Color32::from_rgb(90, 68, 44)
        },
    );
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut render = None;
    let mut seconds = 8.0;
    let mut record = "house".to_owned();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--assets-root" => {
                root = PathBuf::from(args.next().ok_or("--assets-root requires a directory")?)
            }
            "--render" => {
                render = Some(PathBuf::from(
                    args.next().ok_or("--render requires a WAV path")?,
                ))
            }
            "--seconds" => {
                seconds = args
                    .next()
                    .ok_or("--seconds requires a number")?
                    .parse::<f64>()
                    .map_err(|_| "Invalid duration")?
            }
            "--record" => record = args.next().ok_or("--record requires an ID")?,
            "--help" => {
                println!("vinyl-desktop [--assets-root DIRECTORY] [--render OUTPUT.wav --record house --seconds 8]\nSpace: motor; C: cue; B: knock; Z: zoom. Drop audio files to cut a record.");
                return Ok(());
            }
            _ => return Err(format!("Unknown argument: {arg}").into()),
        }
    }
    if let Some(path) = render {
        let loaded = records::load_builtin(&root, &record, None, 7)?;
        let info = audio::render_wav(loaded.record, &path, seconds)?;
        println!(
            "Rendered {} frames at {} Hz; RMS {:.6}, peak {:.6}, clipped {}, solver resets {}",
            info.frames, info.sample_rate, info.rms, info.peak, info.clipped_samples, info.blown
        );
        return Ok(());
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        depth_buffer: 24,
        multisampling: 4,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 820.0])
            .with_min_inner_size([760.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Cozy Vinyl",
        options,
        Box::new(move |cc| Ok(Box::new(VinylApp::new(cc, root)?))),
    )
    .map_err(Into::into)
}
