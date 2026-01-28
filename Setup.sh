#!/bin/bash

# Configuration
PROJECT_DIR="Toolchain/nosman"
OUTPUT_NAME="nodos"
BIN_NAME="nosman"

# Navigate to the project directory
if ! cd "$PROJECT_DIR"; then
    echo "[ERROR] Could not find directory $PROJECT_DIR"
    exit 1
fi

echo "Building nosman"
cargo build --release
if [ $? -ne 0 ]; then
    echo ""
    echo "[ERROR] Build failed"
    exit 1
fi

# Move back to the original root directory and move the file
cd - > /dev/null

# Remove the old file first to prevent "same file" warnings/errors
if [ -f "./$OUTPUT_NAME" ]; then
    rm -f "./$OUTPUT_NAME"
fi

# Move the new binary
echo "Copying $PROJECT_DIR/target/release/$BIN_NAME to ./$OUTPUT_NAME"
mv "$PROJECT_DIR/target/release/$BIN_NAME" "./$OUTPUT_NAME"

# Ensure the new binary is executable
chmod +x "./$OUTPUT_NAME"

echo ""
echo "Success"