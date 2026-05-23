use std::{
    borrow::Cow,
    collections::HashMap,
    env, process,
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::Local;
use iced::widget::{
    Canvas, Shader, Space, canvas, column, container, image, operation::focus, stack, text,
    text_input,
};
use iced::{
    Alignment, Background, Color, ContentFit, Element, Event, Length, Point, Rectangle, Renderer,
    Shadow, Size, Subscription, Task, Theme, Vector, border, event, keyboard, mouse, time, wgpu,
    window,
};
use iced_sessionlock::{actions::UnLockAction, application as sessionlock_application};
use limes_lock::{
    AuthFailure as ProtoAuthFailure, AuthOutcome, AuthRequest, LockRuntime, LockState,
    StderrEventSink,
};

const IDLE_AFTER: Duration = Duration::from_secs(8);
const PASSWORD_INPUT_ID: &str = "password-input";
const SPINNER_PETALS: usize = 12;
const SPINNER_FRAME: Duration = Duration::from_millis(16);
const RESTING_FRAME: Duration = Duration::from_millis(16);
const SPINNER_REVOLUTION: Duration = Duration::from_millis(1100);
const SPINNER_FADE_IN: Duration = Duration::from_millis(180);
const SLOW_AUTH_AFTER: Duration = Duration::from_secs(3);
const PREVIEW_AUTH_DELAY: Duration = Duration::from_millis(900);
const WALLPAPER_BYTES: &[u8] = include_bytes!("../bg.jpg");
const RAIN_INTENSITY: f32 = 0.90;
const INPUT_WIDTH: f32 = 420.0;
const INPUT_HEIGHT: f32 = 64.0;
const CIRCLE_TRANSITION: Duration = Duration::from_millis(260);

fn main() {
    if let Err(error) = run() {
        eprintln!("limes-full-screenlock: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), String> {
    match CliMode::from_args(env::args().skip(1))? {
        Some(CliMode::Lock) => run_lock(),
        Some(CliMode::Preview) => run_preview(),
        None => Ok(()),
    }
}

fn run_lock() -> Result<(), String> {
    let runtime = Arc::new(
        LockRuntime::from_env()
            .map_err(|error| format!("cannot initialize limes lock runtime: {error}"))?,
    );
    runtime.events().subscribe(Arc::new(StderrEventSink));

    sessionlock_application(
        move || FullScreenLock::new_lock(Arc::clone(&runtime)),
        FullScreenLock::update,
        FullScreenLock::view,
    )
    .subscription(FullScreenLock::subscription)
    .run()
    .map_err(|error| error.to_string())
}

fn run_preview() -> Result<(), String> {
    iced::daemon(
        FullScreenLock::new_preview,
        FullScreenLock::update,
        FullScreenLock::view,
    )
    .title("limes full screenlock preview")
    .subscription(FullScreenLock::subscription)
    .run()
    .map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CliMode {
    Lock,
    Preview,
}

impl CliMode {
    fn from_args(mut args: impl Iterator<Item = String>) -> Result<Option<Self>, String> {
        let Some(command) = args.next() else {
            print_help();
            return Ok(None);
        };

        match command.as_str() {
            "lock" => Self::parse_command(Self::Lock, &command, args),
            "preview" => Self::parse_command(Self::Preview, &command, args),
            "help" | "--help" | "-h" => {
                print_help();
                Ok(None)
            }
            other => Err(format!(
                "unknown command `{other}`; try `limes-full-screenlock --help`"
            )),
        }
    }

    fn parse_command(
        mode: Self,
        command: &str,
        args: impl Iterator<Item = String>,
    ) -> Result<Option<Self>, String> {
        let extra = args.collect::<Vec<_>>();
        if extra
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
        {
            print_command_help(mode);
            return Ok(None);
        }
        if !extra.is_empty() {
            return Err(format!(
                "unexpected arguments for `{command}`: {}",
                extra.join(" ")
            ));
        }

        Ok(Some(mode))
    }
}

fn print_help() {
    println!(
        "limes full screenlock\n\n\
Usage:\n  limes-full-screenlock lock\n  limes-full-screenlock preview\n\n\
Commands:\n  lock     Lock the session using Wayland ext-session-lock-v1 surfaces\n  preview  Show the lock UI in a normal window without locking or PAM"
    );
}

fn print_command_help(mode: CliMode) {
    match mode {
        CliMode::Lock => println!(
            "Usage: limes-full-screenlock lock\n\n\
Runs the full-screen session lock frontend on Wayland ext-session-lock-v1\n\
surfaces and authenticates unlock attempts with limes-lock/PAM."
        ),
        CliMode::Preview => println!(
            "Usage: limes-full-screenlock preview\n\n\
Runs the same lock UI in a normal resizable window. It never locks the\n\
session and Enter only plays the authentication animation."
        ),
    }
}

struct FullScreenLock {
    mode: RunMode,
    runtime: Option<Arc<LockRuntime>>,
    wallpaper: iced::widget::image::Handle,
    rain_started: Instant,
    username: String,
    password: String,
    lock_state: LockState,
    screen_state: ScreenState,
    last_input: Instant,
    auth_started: Option<Instant>,
    failure_shade: bool,
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunMode {
    Lock,
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenState {
    Idle,
    Typing,
    Authenticating,
}

#[derive(Debug, Clone)]
enum Message {
    PasswordChanged(String),
    Submit,
    AuthFinished(AuthOutcome),
    PreviewAuthFinished,
    UnlockSession,
    PreviewWindowOpened,
    WindowCloseRequested,
    Tick(Instant),
    IcedEvent(Event),
}

impl TryFrom<Message> for UnLockAction {
    type Error = Message;

    fn try_from(value: Message) -> Result<Self, Self::Error> {
        match value {
            Message::UnlockSession => Ok(UnLockAction),
            other => Err(other),
        }
    }
}

impl FullScreenLock {
    fn new_lock(runtime: Arc<LockRuntime>) -> (Self, Task<Message>) {
        (Self::new(RunMode::Lock, Some(runtime)), Task::none())
    }

    fn new_preview() -> (Self, Task<Message>) {
        let (_, open_window) = window::open(window::Settings {
            size: Size::new(1280.0, 720.0),
            min_size: Some(Size::new(640.0, 360.0)),
            ..window::Settings::default()
        });

        (
            Self::new(RunMode::Preview, None),
            open_window.map(|_| Message::PreviewWindowOpened),
        )
    }

    fn new(mode: RunMode, runtime: Option<Arc<LockRuntime>>) -> Self {
        let now = Instant::now();

        Self {
            mode,
            runtime,
            wallpaper: iced::widget::image::Handle::from_bytes(WALLPAPER_BYTES),
            rain_started: now,
            username: env::var("USER").unwrap_or_default(),
            password: String::new(),
            lock_state: LockState::Locked,
            screen_state: ScreenState::Idle,
            last_input: now,
            auth_started: None,
            failure_shade: false,
            status: if mode == RunMode::Preview {
                "Preview mode: PAM is not called.".to_owned()
            } else {
                String::new()
            },
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        let frame_time = if self.screen_state == ScreenState::Authenticating {
            SPINNER_FRAME
        } else {
            RESTING_FRAME
        };

        let mut subscriptions = vec![
            event::listen().map(Message::IcedEvent),
            time::every(frame_time).map(Message::Tick),
        ];

        if self.mode == RunMode::Preview {
            subscriptions.push(window::close_requests().map(|_| Message::WindowCloseRequested));
        }

        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PasswordChanged(password) => {
                self.password = password;
                self.failure_shade = false;
                self.status.clear();
                self.activate_typing()
            }
            Message::Submit => self.submit(),
            Message::AuthFinished(outcome) => self.finish_auth(outcome),
            Message::PreviewAuthFinished => self.finish_preview_auth(),
            Message::UnlockSession | Message::PreviewWindowOpened => Task::none(),
            Message::WindowCloseRequested => iced::exit(),
            Message::Tick(now) => {
                if self.screen_state == ScreenState::Typing
                    && now.duration_since(self.last_input) > IDLE_AFTER
                {
                    self.password.clear();
                    self.failure_shade = false;
                    self.status.clear();
                    self.screen_state = ScreenState::Idle;
                }
                Task::none()
            }
            Message::IcedEvent(event) => match event {
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    text: typed_text,
                    ..
                }) => match key {
                    keyboard::Key::Named(keyboard::key::Named::Enter) => self.submit(),
                    keyboard::Key::Named(keyboard::key::Named::Escape) => {
                        self.password.clear();
                        self.failure_shade = false;
                        self.status.clear();
                        self.screen_state = ScreenState::Idle;
                        Task::none()
                    }
                    _ => {
                        if self.screen_state == ScreenState::Idle {
                            if let Some(typed_text) = typed_text {
                                let typed_text: &str = typed_text.as_ref();

                                if !typed_text.is_empty()
                                    && typed_text.chars().all(|character| !character.is_control())
                                {
                                    self.password.push_str(typed_text);
                                    self.failure_shade = false;
                                    self.status.clear();
                                }
                            }
                        }

                        self.activate_typing()
                    }
                },
                Event::Mouse(mouse::Event::ButtonPressed(_)) => self.activate_typing(),
                _ => Task::none(),
            },
        }
    }

    fn activate_typing(&mut self) -> Task<Message> {
        if self.screen_state != ScreenState::Authenticating {
            self.screen_state = ScreenState::Typing;
            self.last_input = Instant::now();
            focus(PASSWORD_INPUT_ID)
        } else {
            Task::none()
        }
    }

    fn submit(&mut self) -> Task<Message> {
        if self.screen_state == ScreenState::Authenticating || self.lock_state != LockState::Locked
        {
            return Task::none();
        }

        // Allow an empty secret to reach PAM so passwordless modules (for
        // example fingerprint auth) can start from an Enter-only submit.
        let mut password = std::mem::take(&mut self.password);

        self.failure_shade = false;
        self.screen_state = ScreenState::Authenticating;
        self.lock_state = LockState::Unlocking;
        self.auth_started = Some(Instant::now());

        match self.mode {
            RunMode::Lock => {
                let Some(runtime) = self.runtime.as_ref().map(Arc::clone) else {
                    self.lock_state = LockState::Locked;
                    self.screen_state = ScreenState::Typing;
                    self.auth_started = None;
                    self.failure_shade = true;
                    self.status = "Lock runtime unavailable.".to_owned();
                    return focus(PASSWORD_INPUT_ID);
                };
                let username = self.username.clone();
                self.status = "Verifying with PAM…".to_owned();

                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut request = AuthRequest::new(username, password);
                            let outcome = runtime.authenticate_unlock(&request);
                            request.clear_secret();
                            outcome
                        })
                        .await
                        .unwrap_or_else(|error| {
                            Err(ProtoAuthFailure::Internal(format!(
                                "authentication task failed: {error}"
                            )))
                        })
                    },
                    Message::AuthFinished,
                )
            }
            RunMode::Preview => {
                password.clear();
                self.status = "Preview mode: simulating authentication…".to_owned();

                Task::perform(
                    async {
                        tokio::time::sleep(PREVIEW_AUTH_DELAY).await;
                    },
                    |_| Message::PreviewAuthFinished,
                )
            }
        }
    }

    fn finish_auth(&mut self, outcome: AuthOutcome) -> Task<Message> {
        match outcome {
            Ok(_) => {
                self.lock_state = LockState::Unlocked;
                self.auth_started = None;
                self.status = "Unlocked.".to_owned();
                Task::done(Message::UnlockSession)
            }
            Err(error) => {
                self.lock_state = LockState::Locked;
                self.screen_state = ScreenState::Typing;
                self.auth_started = None;
                self.failure_shade = true;
                self.status = auth_error_message(&error);
                self.last_input = Instant::now();
                focus(PASSWORD_INPUT_ID)
            }
        }
    }

    fn finish_preview_auth(&mut self) -> Task<Message> {
        self.lock_state = LockState::Locked;
        self.screen_state = ScreenState::Typing;
        self.auth_started = None;
        self.failure_shade = false;
        self.status = "Preview mode: authentication skipped.".to_owned();
        self.last_input = Instant::now();
        focus(PASSWORD_INPUT_ID)
    }

    fn view(&self, window: window::Id) -> Element<'_, Message> {
        // Keep a stable image handle. Recreating `Handle::from_bytes` in every
        // view pass gives iced a fresh cache id each frame and can leave the
        // wallpaper blank while the lock screen is redrawing.
        let background = image(self.wallpaper.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .content_fit(ContentFit::Cover);

        let rain = Shader::new(RainDrops {
            window,
            time: self.rain_started.elapsed().as_secs_f32(),
            intensity: RAIN_INTENSITY,
        })
        .width(Length::Fill)
        .height(Length::Fill);

        let content = match self.screen_state {
            ScreenState::Idle => self.idle_view(),
            ScreenState::Typing => self.typing_view(false),
            ScreenState::Authenticating => self.typing_view(true),
        };

        stack![background, rain, content].into()
    }

    fn idle_view(&self) -> Element<'_, Message> {
        let now = Local::now();
        let clock = column![
            text(now.format("%H:%M").to_string())
                .size(112)
                .color(Color::WHITE),
            text(now.format("%A, %B %-d").to_string())
                .size(25)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.86)),
        ]
        .align_x(Alignment::Center)
        .spacing(2);

        let glass_clock = container(clock)
            .padding([24, 48])
            .width(Length::Shrink)
            .style(|_| clock_glass());

        container(column![
            Space::new().height(Length::Fixed(56.0)),
            glass_clock
        ])
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Center)
        .into()
    }

    fn typing_view(&self, loading: bool) -> Element<'_, Message> {
        let field: Element<'_, Message> = if loading {
            let elapsed = self
                .auth_started
                .map(|started| started.elapsed())
                .unwrap_or(Duration::ZERO);
            let morph = loading_morph(self.auth_started);
            let width = lerp(INPUT_WIDTH, INPUT_HEIGHT, morph);

            container(
                Canvas::new(FlowerSpinner { elapsed })
                    .width(Length::Fixed(44.0))
                    .height(Length::Fixed(44.0)),
            )
            .width(Length::Fixed(width))
            .height(Length::Fixed(INPUT_HEIGHT))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(|_| input_shell(false))
            .into()
        } else {
            let failed = self.failure_shade;
            let placeholder = if !self.status.is_empty() {
                self.status.as_str()
            } else {
                "Password"
            };

            let input = text_input(placeholder, &self.password)
                .id(PASSWORD_INPUT_ID)
                .on_input(Message::PasswordChanged)
                .on_submit(Message::Submit)
                .secure(true)
                .padding([14, 22])
                .size(22)
                .width(Length::Fill)
                .style(move |_, status| password_input_style(status, failed));

            container(input)
                .width(Length::Fixed(INPUT_WIDTH))
                .height(Length::Fixed(INPUT_HEIGHT))
                .align_y(Alignment::Center)
                .style(move |_| input_shell(failed))
                .into()
        };

        let content = if loading {
            let pam_status = container(
                text(auth_status_message(self.auth_started, &self.status))
                    .size(15)
                    .color(Color::from_rgba(1.0, 1.0, 1.0, 0.74)),
            )
            .padding([8, 18])
            .width(Length::Shrink)
            .style(|_| input_shell(false));

            column![
                Space::new().height(Length::Fill),
                field,
                pam_status,
                Space::new().height(Length::Fixed(54.0))
            ]
            .align_x(Alignment::Center)
            .spacing(12)
        } else {
            column![
                Space::new().height(Length::Fill),
                field,
                Space::new().height(Length::Fixed(84.0))
            ]
            .align_x(Alignment::Center)
        };

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .into()
    }
}

#[derive(Debug, Clone, Copy)]
struct FlowerSpinner {
    elapsed: Duration,
}

impl<Message> canvas::Program<Message> for FlowerSpinner {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let center = frame.center();
        let elapsed = self.elapsed.as_secs_f32();
        let phase = (elapsed / SPINNER_REVOLUTION.as_secs_f32()).fract();
        let rotation = phase * std::f32::consts::TAU;
        let appear = ease_out_cubic((elapsed / SPINNER_FADE_IN.as_secs_f32()).clamp(0.0, 1.0));
        let pulse = 1.0 + (elapsed * std::f32::consts::TAU * 1.35).sin() * 0.025;
        let scale = lerp(0.84, 1.0, appear) * pulse;

        let glow = canvas::Path::circle(center, 19.0 * scale);
        frame.fill(&glow, Color::from_rgba(0.58, 0.74, 1.0, 0.08 * appear));

        let ring = canvas::Path::circle(center, 20.0 * scale);
        frame.stroke(
            &ring,
            canvas::Stroke::default()
                .with_width(1.0)
                .with_color(Color::from_rgba(1.0, 1.0, 1.0, 0.12 * appear)),
        );

        frame.with_save(|frame| {
            frame.translate(Vector::new(center.x, center.y));
            frame.scale(scale);

            for petal in 0..SPINNER_PETALS {
                let trail = petal as f32 / SPINNER_PETALS as f32;
                let intensity = (1.0 - trail).powf(2.35);
                let alpha = (0.14 + intensity * 0.76) * appear;
                let angle = rotation - std::f32::consts::TAU * trail;
                let width = lerp(3.1, 5.0, intensity);
                let length = lerp(9.5, 15.5, intensity);
                let offset = lerp(17.0, 22.5, intensity);
                let color = Color::from_rgba(
                    lerp(0.72, 1.0, intensity),
                    lerp(0.84, 1.0, intensity),
                    1.0,
                    alpha,
                );

                frame.with_save(|frame| {
                    frame.rotate(angle);

                    let petal = canvas::Path::rounded_rectangle(
                        Point::new(-width / 2.0, -offset),
                        Size::new(width, length),
                        (width / 2.0).into(),
                    );

                    frame.fill(&petal, color);
                });
            }
        });

        let core = canvas::Path::circle(center, 2.5 * scale);
        frame.fill(&core, Color::from_rgba(1.0, 1.0, 1.0, 0.52 * appear));

        vec![frame.into_geometry()]
    }
}

#[derive(Debug, Clone, Copy)]
struct RainDrops {
    window: window::Id,
    time: f32,
    intensity: f32,
}

impl<Message> iced::widget::shader::Program<Message> for RainDrops {
    type State = ();
    type Primitive = RainDropsPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        RainDropsPrimitive {
            window: self.window,
            time: self.time,
            intensity: self.intensity,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct RainDropsPrimitive {
    window: window::Id,
    time: f32,
    intensity: f32,
}

impl iced::widget::shader::Primitive for RainDropsPrimitive {
    type Pipeline = RainDropsPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        pipeline.prepare(self, device, queue, bounds, viewport);
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.draw(self, render_pass)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RainDropsUniform {
    resolution: [f32; 2],
    time: f32,
    intensity: f32,
}

struct RainDropsBindings {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

impl RainDropsBindings {
    fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("limes rain-drop uniforms"),
            size: std::mem::size_of::<RainDropsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("limes rain-drop bind group"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self { buffer, bind_group }
    }
}

struct RainDropsPipeline {
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    uniforms: HashMap<window::Id, RainDropsBindings>,
}

impl RainDropsPipeline {
    fn prepare(
        &mut self,
        primitive: &RainDropsPrimitive,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &iced::widget::shader::Viewport,
    ) {
        let scale_factor = viewport.scale_factor();
        let uniforms = RainDropsUniform {
            resolution: [
                (bounds.width * scale_factor).max(1.0),
                (bounds.height * scale_factor).max(1.0),
            ],
            time: primitive.time,
            intensity: primitive.intensity.clamp(0.0, 1.0),
        };

        let bindings = self.bindings(device, primitive.window);
        queue.write_buffer(&bindings.buffer, 0, bytemuck::bytes_of(&uniforms));
    }

    fn draw(&self, primitive: &RainDropsPrimitive, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(bindings) = self.uniforms.get(&primitive.window) else {
            return false;
        };

        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &bindings.bind_group, &[]);
        render_pass.draw(0..3, 0..1);

        true
    }

    fn bindings(&mut self, device: &wgpu::Device, window: window::Id) -> &RainDropsBindings {
        if !self.uniforms.contains_key(&window) {
            let bindings = RainDropsBindings::new(device, &self.bind_group_layout);
            self.uniforms.insert(window, bindings);
        }

        self.uniforms
            .get(&window)
            .expect("rain-drop uniforms should be initialized")
    }
}

impl iced::widget::shader::Pipeline for RainDropsPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("limes rain-drop bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<RainDropsUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("limes rain-drop pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("limes rain-drop shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("rain_drops.wgsl"))),
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("limes rain-drop pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview: None,
            cache: None,
        });

        Self {
            render_pipeline,
            bind_group_layout,
            uniforms: HashMap::new(),
        }
    }
}

fn auth_status_message<'a>(auth_started: Option<Instant>, status: &'a str) -> &'a str {
    if auth_started.is_some_and(|started| started.elapsed() > SLOW_AUTH_AFTER) {
        "Still verifying…"
    } else if status.is_empty() {
        "Verifying…"
    } else {
        status
    }
}

fn loading_morph(auth_started: Option<Instant>) -> f32 {
    auth_started
        .map(|started| {
            let progress =
                (started.elapsed().as_secs_f32() / CIRCLE_TRANSITION.as_secs_f32()).clamp(0.0, 1.0);
            ease_out_cubic(progress)
        })
        .unwrap_or(1.0)
}

fn ease_out_cubic(progress: f32) -> f32 {
    1.0 - (1.0 - progress).powi(3)
}

fn lerp(from: f32, to: f32, progress: f32) -> f32 {
    from + (to - from) * progress
}

fn clock_glass() -> container::Style {
    container::Style::default()
        .background(Background::Color(Color::from_rgba(
            0.01, 0.014, 0.022, 0.62,
        )))
        .border(
            border::rounded(36.0)
                .width(1.0)
                .color(Color::from_rgba(1.0, 1.0, 1.0, 0.22)),
        )
        .shadow(Shadow {
            color: Color::from_rgba(0.0, 0.0, 0.0, 0.52),
            offset: Vector::new(0.0, 18.0),
            blur_radius: 42.0,
        })
}

fn input_shell(failed: bool) -> container::Style {
    let (background, border_color, shadow_color) = if failed {
        (
            Color::from_rgba(0.22, 0.01, 0.02, 0.66),
            Color::from_rgba(1.0, 0.50, 0.56, 0.68),
            Color::from_rgba(0.18, 0.0, 0.0, 0.52),
        )
    } else {
        (
            Color::from_rgba(0.01, 0.014, 0.022, 0.62),
            Color::from_rgba(1.0, 1.0, 1.0, 0.23),
            Color::from_rgba(0.0, 0.0, 0.0, 0.52),
        )
    };

    container::Style::default()
        .background(Background::Color(background))
        .border(border::rounded(32.0).width(1.0).color(border_color))
        .shadow(Shadow {
            color: shadow_color,
            offset: Vector::new(0.0, 14.0),
            blur_radius: 34.0,
        })
}

fn password_input_style(_status: text_input::Status, failed: bool) -> text_input::Style {
    text_input::Style {
        background: Background::Color(Color::from_rgba(0.0, 0.0, 0.0, 0.0)),
        border: border::rounded(28.0)
            .width(0.0)
            .color(Color::from_rgba(0.0, 0.0, 0.0, 0.0)),
        icon: Color::WHITE,
        placeholder: if failed {
            Color::from_rgba(1.0, 0.82, 0.84, 0.92)
        } else {
            Color::from_rgba(1.0, 1.0, 1.0, 0.58)
        },
        value: Color::WHITE,
        selection: Color::from_rgba(0.38, 0.70, 1.0, 0.42),
    }
}

fn auth_error_message(error: &ProtoAuthFailure) -> String {
    match error {
        ProtoAuthFailure::InvalidCredentials => "Invalid password. Try again.".to_owned(),
        ProtoAuthFailure::LockedOut => "Account is locked out.".to_owned(),
        ProtoAuthFailure::BackendUnavailable(reason) => {
            format!("PAM backend unavailable: {reason}")
        }
        ProtoAuthFailure::Internal(reason) => format!("Authentication error: {reason}"),
    }
}
