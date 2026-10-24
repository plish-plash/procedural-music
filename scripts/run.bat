@echo off
rem Builds (if needed) and starts the desktop app.
cd /d "%~dp0.."
cargo run --manifest-path app\Cargo.toml
if errorlevel 1 pause
