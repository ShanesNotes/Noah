Noah: Critical Care Nurse AI Agent Toolkit with Flood EHR
Introduction
Hey there, I’m Shane McCusker, a critical care nurse with 14 years under my belt at a Level 1 trauma center. I’ve spent over a decade in the ICU—intubating patients, titrating drips, charting vitals at 2 a.m., and training new RNs to survive the chaos of critical care. I built Noah because I’m tired of clunky, top-down hospital software that feels like it was designed by suits who’ve never held a stethoscope. This isn’t some off-the-shelf EHR or AI gimmick—it’s a toolkit forged from the ground up by a nurse who lives this job every day. Noah is trained like I train my ICU RNs: to be sharp, reliable, and ready for anything.
Noah is a Rust-based AI assistant that monitors vitals, manages orders, tracks labs and meds, and logs everything securely—think of it as your extra set of hands in the unit. It’s got a real-time pipeline for streaming data, a simple UI called Flood EHR, and even blockchain logging for accountability (because paper trails matter). I built it to cut through the noise and help us focus on what we do best: saving lives.
Why Noah?
In the ICU, every second counts. I’ve seen software slow us down—freezing during a code, burying critical info under ten menus, or spitting out alerts nobody asked for. Noah’s different. It’s:
Useful: Checks vitals, flags thresholds (like MAP < 65), and suggests interventions—like titrating Levophed or pushing potassium—based on real ICU logic I’ve honed over years.
Nurse-Driven: I coded it to think like we do. It prioritizes tasks by severity, not some algorithm’s guess, and speaks our language (e.g., “HR 72, SpO2 98, MAP 93”).
Built, Not Bought: No vendor BS. I wrote this in Rust for speed and reliability—because downtime isn’t an option when a patient’s crashing.
Bottom Up: This isn’t a corporate mandate. It’s born from the bedside, shaped by the grind of night shifts and the needs of my team.
What Can Noah Do?
Monitor Vitals: Streams data from a WebSocket (e.g., bedside monitors) and alerts on thresholds—like low MAP or high lactic acid.
Manage Orders: Tracks order sets (e.g., fluids, CRRT) and flags when vitals or I/O don’t match the plan.
Chart Labs & Meds: Updates labs (e.g., K+, lactic acid) and MAR (e.g., Levophed doses) with blockchain logging for an tamper-proof record.
Flood EHR UI: A clean, tabbed interface (Vitals, Labs, Meds, Orders, I/O) to see what’s what without digging.
Task Queue: Prioritizes interventions (severity 1-10) and executes them—like “Administer 20 mEq potassium”—when due.
Scalability: Handles multiple patients with a worker pool and Redis caching, because ICUs don’t run single-file.
Getting Started
Noah’s built to run in a Dockerized environment with SQLite, Redis, and a WebSocket server for vitals. Here’s how to fire it up:
Prerequisites
Rust 1.80+ (for the 2021 edition)
Docker (optional, but recommended)
Redis (redis:6379)
A WebSocket server (e.g., mock it at ws://localhost:8080/vitals)
Installation
Clone the Repo:
bash
git clone https://github.com/shanesnotes/noah.git
cd noah
Build It:
bash
cargo build --release
Set Up Environment:
Create a .env file:
env
RUST_LOG=info
Run Noah:
Monitor vitals for a patient:
bash
cargo run --release -- --command monitor-vitals --patient-id "JohnDoe"
Output: Vitals for nurse review: HR: 72, SpO2: 98, MAP: 93, RR: 18, Temp: 38.2
Stream vitals:
bash
cargo run --release -- --command monitor-vitals-stream --patient-id "JohnDoe"
Launch the full system (vitals pipeline + API):
bash
cargo run --release -- --command launch-flood
API runs at http://0.0.0.0:8081, metrics at http://0.0.0.0:9090.
Docker (Recommended)
Build the image:
bash
docker build -t noah .
Run with Redis:
bash
docker run -d --name redis redis
docker run -p 8081:8081 -p 9090:9090 --link redis:redis noah --command launch-flood
Usage Examples
Check Vitals: cargo run -- --command monitor-vitals --patient-id "JaneDoe"
Analyze Labs: cargo run -- --command analyze-labs --patient-id "JaneDoe" (e.g., flags K+ < 3.5).
View API: Hit http://localhost:8081/vitals/JaneDoe for JSON vitals.
Flood UI: Enable the iced feature (cargo run --features iced) for the desktop app.
How It’s Trained
Noah learns like my RNs: through practical, real-world scenarios. Its tools—like VitalsChecker or AdministerMedication—mimic how I’d walk a newbie through a shift. For example:
Vitals: Hardcoded to return realistic ICU values (HR 72, MAP 93) until we hook up real monitors.
Alerts: If MAP drops below an order’s threshold, it queues “Titrate Levophed” with severity 8—because that’s what I’d do.
Blockchain: Logs every action (e.g., med administration) with a hash, like a digital MAR I can trust.
Future versions will train on anonymized patient data (with IRB approval) to refine thresholds and interventions—think of it as a resident getting better with every case.
Contributing
This is open-source under MIT. If you’re a nurse, dev, or both, jump in:
Nurses: Tell me what sucks about your EHR—let’s fix it.
Devs: PRs welcome for MongoDB/Postgres support, better UI, or real blockchain integration (e.g., Ethereum).
File issues or hit me up at smccusker22@gmail.com. Let’s make Noah the teammate we all wish we had.
Roadmap
Real Data: Swap mock tools for live feeds (e.g., Philips monitors).
Voice Commands: --voice flag will trigger text-to-speech for hands-free use.
Analytics: Clickhouse for trending vitals over time.
Mobile App: Port Flood EHR to iOS/Android.
Acknowledgments
To my ICU crew—thanks for the grit that inspired this. To Rust’s community—your tools made it possible. Built not bought, from the bottom up.