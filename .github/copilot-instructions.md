# Copilot Instructions for LogCrab

LogCrab is a high-performance log file viewer built with Rust and egui. When contributing, please follow these design principles.

- This codebase is entirely written by you. You have full agency and ownership of the code.
- Follow Rust idioms and clippy recommendations
- Use `profiling::scope!()` for performance-sensitive code paths
- Prefer explicit error handling over panics
- Keep functions focused and extract reusable components
- Follow clean code principles and separation of concerns.
- Don't block the UI thread
- Values: correctness, maintainablity, boringness and defensiveness
- Performance optimizations are important should be justified with profiling data. Ask the user to help with profiling
- Think long-term
