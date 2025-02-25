# Noah: AI-Powered Critical Care Nurse Assistant

## Built by nurses, for nurses

Noah is an AI nurse assistant designed from the ground up by Shane McCusker, a veteran critical care nurse with 14 years of experience in a Level 1 trauma center. Unlike corporate healthcare IT solutions, Noah was born at the bedside—created to solve real problems faced by frontline healthcare workers.

## What makes Noah different?

**Bottom-up design:** Created by a practicing ICU nurse, not corporate executives
**Practical functionality:** Focuses on what matters in critical care—monitoring vitals, managing orders, tracking labs and medications
**Nurse-driven logic:** Thinks like an experienced nurse, prioritizing tasks by clinical severity
**Built for speed:** Written in Rust for reliability and performance when every second counts

## Core capabilities

- **Real-time vital monitoring:** Tracks critical parameters and alerts on clinically significant thresholds
- **Intelligent order management:** Ensures care plans are followed and interventions are timely
- **Secure documentation:** Records all clinical data with tamper-proof blockchain logging
- **Flood EHR:** A clean, intuitive interface designed for high-stress environments
- **Task prioritization:** Organizes interventions by clinical urgency, just like an experienced nurse would

## Technical foundation

Noah is built on a modern, robust technology stack:
- Rust backend for speed and reliability
- WebSocket integration for real-time data streaming
- Modular architecture for extensibility
- Clean, minimalist web interface

## Getting started

```bash
# Clone the repository
git clone https://github.com/shanesnotes/noah.git
cd noah

# Build the project
cargo build --release

# Run Noah
cargo run --release -- --command launch-flood
```

Visit `http://localhost:8081` to access the Flood EHR interface.

## Vision

Noah represents a paradigm shift in healthcare technology—moving away from top-down corporate solutions to tools built by clinicians who understand the realities of patient care. By combining clinical expertise with modern software engineering, Noah aims to reduce cognitive burden, minimize errors, and ultimately improve patient outcomes.

## Contributing

This project welcomes contributions from nurses, developers, and anyone passionate about improving healthcare technology. Whether you're sharing clinical insights or technical expertise, there's a place for you in building Noah.

## License

MIT License

---

*"I built Noah because I'm tired of clunky, top-down hospital software that feels like it was designed by suits who've never held a stethoscope. This is a toolkit forged from the ground up by a nurse who lives this job every day."* — Shane McCusker, RN
