use iced::widget::{pick_list, column, text, button, row, Scrollable, Container};
use iced::{Application, Command, Element, Theme, executor, Length};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use futures_util::StreamExt;
use serde_json::Value;
use dashmap::DashMap;
use crate::core::ai::AiService;
use crate::models::patient::PatientData;

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(String),
    UpdateData(String, String),
    ModelSelected(String),
    RequestAiAnalysis,
    AiAnalysisReceived(Result<String, String>),
}

pub struct FloodApp {
    ai_service: Arc<AiService>,
    selected_tab: String,
    patient_data: DashMap<String, String>,
    model_options: Vec<String>,
    selected_model: String,
    message_rx: mpsc::Receiver<Message>,
    ai_analysis: Option<String>,
}

impl Application for FloodApp {
    type Executor = executor::Default;
    type Message = Message;
    type Theme = Theme;
    type Flags = Arc<AiService>;

    fn new(ai_service: Self::Flags) -> (Self, Command<Self::Message>) {
        let (tx, rx) = mpsc::channel(100);
        let ai_service_clone = ai_service.clone();
        
        // Start WebSocket connection for real-time data
        tokio::spawn(async move {
            let (mut ws_stream, _) = connect_async("ws://localhost:8080/vitals").await.unwrap();
            while let Some(message) = ws_stream.next().await {
                if let Ok(msg) = message {
                    if let Ok(text) = msg.to_text() {
                        if let Ok(data) = serde_json::from_str::<Value>(text) {
                            if let Some(vitals) = data["vitals"].as_object() {
                                let vitals_str = vitals.iter()
                                    .map(|(k, v)| format!("{}: {}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                tx.send(Message::UpdateData("vitals".to_string(), vitals_str)).await.unwrap();
                            }
                        }
                    }
                }
            }
        });

        let model_options = vec!["Claude".to_string(), "GPT-4".to_string()];
        (
            Self {
                ai_service,
                selected_tab: "Vitals".to_string(),
                patient_data: DashMap::new(),
                model_options,
                selected_model: "Claude".to_string(),
                message_rx: rx,
                ai_analysis: None,
            },
            Command::none()
        )
    }

    fn title(&self) -> String {
        "Noah - Flood EHR".to_string()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Message::TabSelected(tab) => {
                self.selected_tab = tab;
                Command::none()
            },
            Message::UpdateData(key, value) => {
                self.patient_data.insert(key, value);
                Command::none()
            },
            Message::ModelSelected(model) => {
                self.selected_model = model;
                Command::none()
            },
            Message::RequestAiAnalysis => {
                let ai_service = self.ai_service.clone();
                let vitals_data = self.patient_data.get("vitals").map(|v| v.to_string()).unwrap_or_default();
                
                Command::perform(
                    async move {
                        // Convert vitals string to HashMap
                        let vitals = crate::core::data::process_vitals_data(&vitals_data).unwrap_or_default();
                        ai_service.analyze_vitals(&vitals).await.map_err(|e| e.to_string())
                    },
                    Message::AiAnalysisReceived
                )
            },
            Message::AiAnalysisReceived(result) => {
                self.ai_analysis = match result {
                    Ok(analysis) => Some(analysis),
                    Err(err) => Some(format!("Error: {}", err)),
                };
                Command::none()
            }
        }
    }

    fn view(&self) -> Element<Self::Message> {
        // Header with title and model selector
        let header = row![
            text("Noah - Flood EHR").size(28),
            pick_list(
                &self.model_options,
                Some(self.selected_model.clone()),
                Message::ModelSelected,
            ),
        ].spacing(20).padding(20).align_items(iced::Alignment::Center);
        
        // Tab navigation
        let tabs = row![
            button("Vitals").on_press(Message::TabSelected("Vitals".to_string())),
            button("Labs").on_press(Message::TabSelected("Labs".to_string())),
            button("Meds").on_press(Message::TabSelected("Meds".to_string())),
            button("Orders").on_press(Message::TabSelected("Orders".to_string())),
            button("I/O").on_press(Message::TabSelected("I/O".to_string())),
        ].spacing(10).padding(10);

        // Main content based on selected tab
        let content = match self.selected_tab.as_str() {
            "Vitals" => self.view_vitals(),
            "Labs" => self.view_labs(),
            "Meds" => self.view_medications(),
            "Orders" => self.view_orders(),
            "I/O" => self.view_io(),
            _ => text("Select a tab").into(),
        };

        Container::new(column![
            header,
            tabs,
            content
        ].spacing(20))
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(20)
            .into()
    }
    
    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::subscription::channel(
            std::any::TypeId::of::<Self>(),
            100,
            |mut sender| {
                let mut rx = self.message_rx.clone();
                async move {
                    while let Some(msg) = rx.recv().await {
                        sender.send(msg).await.unwrap();
                    }
                    iced::futures::future::pending().await
                }
            }
        )
    }
}

impl FloodApp {
    // Helper methods for different views
    fn view_vitals(&self) -> Element<Message> {
        let vitals_data = self.patient_data.get("vitals")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "No vitals data available".to_string());
            
        let vitals_display = column![
            text("Current Vitals").size(24),
            text(&vitals_data).size(18),
            button("Request AI Analysis").on_press(Message::RequestAiAnalysis),
        ].spacing(10);
        
        let ai_analysis = if let Some(analysis) = &self.ai_analysis {
            column![
                text("AI Analysis").size(24),
                text(analysis).size(18),
            ].spacing(10)
        } else {
            column![
                text("AI Analysis").size(24),
                text("Click 'Request AI Analysis' to get insights").size(18),
            ].spacing(10)
        };
        
        Scrollable::new(
            column![
                vitals_display,
                ai_analysis,
            ].spacing(20)
        ).into()
    }
    
    fn view_labs(&self) -> Element<Message> {
        let labs_data = self.patient_data.get("labs")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "No lab data available".to_string());
            
        Scrollable::new(
            column![
                text("Laboratory Results").size(24),
                text(&labs_data).size(18),
            ].spacing(10)
        ).into()
    }
    
    fn view_medications(&self) -> Element<Message> {
        let meds_data = self.patient_data.get("meds")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "No medication data available".to_string());
            
        Scrollable::new(
            column![
                text("Medications").size(24),
                text(&meds_data).size(18),
            ].spacing(10)
        ).into()
    }
    
    fn view_orders(&self) -> Element<Message> {
        let orders_data = self.patient_data.get("orders")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "No orders data available".to_string());
            
        Scrollable::new(
            column![
                text("Orders").size(24),
                text(&orders_data).size(18),
            ].spacing(10)
        ).into()
    }
    
    fn view_io(&self) -> Element<Message> {
        let io_data = self.patient_data.get("io")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "No I/O data available".to_string());
            
        Scrollable::new(
            column![
                text("Input/Output").size(24),
                text(&io_data).size(18),
            ].spacing(10)
        ).into()
    }
} 