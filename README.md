# 🌎 Model UN - Consensus Building for Agile Teams

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Rust-DEA584?style=flat&logo=rust&logoColor=black)](https://www.rust-lang.org/)
[![Warp](https://img.shields.io/badge/Warp-WebSockets-blue)](https://github.com/seanmonstar/warp)
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](http://makeapullrequest.com)

## 🇺🇳 What is Model UN?
WTF even is a Planning Poker? Poker is a fun card game, not a serious business tool!

Model UN is an open-source, real-time consensus-building tool that merges the formality of United Nations proceedings with the practicality of agile planning poker. 
This game offers teams a fun, themed way to estimate story points, prioritize work items, or vote on any team decision that benefits from anonymous input and collective reveal.

Planning poker looks white and sterile and boring, we look cool AF. and We got more than two colors!

Perfect for:
- Remote sprint planning sessions
- Effort estimation ceremonies
- Prioritization meetings
- Team-wide decision making
- Actual Model UN simulations (political science educators rejoice!)

## 🗳️ Features

- **Real-time communication** through WebSockets for instant updates
- **Room-based sessions** - different teams can use separate virtual chambers using the url <url>/index.html?room=<WhateverStringYouWant>
- **Auto-delegation** - spectators automatically fill vacant seats when delegates leave
- **Fibonacci sequence** voting options (1, 2, 3, 5, 8, 13, 21) : In the future I'd like to support more sequences.
- **Visual indicators** showing which delegates have cast their votes
- **Confirmation prompts** when revealing with incomplete votes
- **Reset functionality** for multiple voting rounds

## 🚀 Quick Start

1. Clone this repository:
```bash
git clone https://github.com/your-username/model-un.git
cd model-un
```

2. Start the Rust server:
```bash
# Install Rust if needed (https://rustup.rs/)
cargo run
```
add `RUST_LOG=<debug | info | warn | error>` to the command above to set the logging level

3. Open your browser to `http://localhost:3000`

4. The app will automatically create a new room and redirect you to the room!

5. Have friends join a room by navigating to `http://URL:3000/index.html?room=your-room-name`
   - We are assuming that your firewall port is open

5. Begin diplomatic negotiations (aka estimation)!


## 🔧 Technical Architecture

### Frontend
- Vanilla JavaScript client with WebSocket communication.
- Bare bones DOM manipulation BABBBYYY! No frameworks.
- Clean CSS styling with video game like asthetics.
- Responsive design for various device sizes.
- Low level design with NO external dependencies.

### Backend
- **Rust** server using the **Warp** framework
- WebSocket-based real-time state synchronization
- Room-based session management
- Broadcast channels for efficient message distribution
- Thread-safe shared state with Arc and Mutex


## 🤝 Contributing

The General Assembly welcomes contributions from all nations! Whether you're fixing bugs, adding features, or improving documentation - your diplomatic efforts strengthen our global codebase.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Resolution: Add amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

NOTE! Make sure that your PR passes the CI/CD pipeline.
Run `cargo clippy` and `cargo fmt` to ensure your code is up to the Rust standards.

### Contribution Ideas
- Mobile-responsive design improvements
- Additional voting schemes (beyond Fibonacci)
- Voting analytics and statistical summaries
- Custom delegate avatars and country flags
- Integration with project management tools
- Internationalization support
- Performance optimizations for large assemblies

## 🛠️ Development Setup

### Prerequisites
- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [Docker](https://docs.docker.com/get-docker/) (for deployment)
- [Firefox](https://www.mozilla.org/en-US/firefox/new/) or [Chrome](https://www.google.com/chrome/) browser

### Environment Variables
- `LOG_LEVEL`: Logging verbosity (default: info)

## 📋 Code of Conduct

1. Be respectful to fellow delegates
2. Maintain diplomatic immunity (but not for bad code)
3. Ensure your code passes the Security Council review (our CI/CD pipeline)
4. No vetoing legitimate bug fixes!

## 📝 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🌟 Acknowledgments

- Inspired by UN procedures, Civilization, and a hatred of planning poker
- Built with Rust for reliability and performance
- Thanks to all the scrum masters who moderate like seasoned diplomats
- And to product owners who set agendas worthy of the Security Council

---

### "This is how democracy dies, with thunderous applause!" 🕊️
