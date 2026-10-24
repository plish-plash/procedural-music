@echo off
rem Builds and tests everything, writing the output to build-log.txt in the project folder.
cd /d "%~dp0.."
> build-log.txt echo === build started %date% %time%
echo --- engine and theory tests >> build-log.txt
cargo test --color never --workspace >> build-log.txt 2>&1
echo TESTS EXIT %ERRORLEVEL% >> build-log.txt
echo --- desktop app >> build-log.txt
cargo build --color never --manifest-path app\Cargo.toml >> build-log.txt 2>&1
echo BUILD EXIT %ERRORLEVEL% >> build-log.txt
echo === build finished %date% %time% >> build-log.txt
