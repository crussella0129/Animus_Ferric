# Build multi-arch images for ferric-core
# Requirements: Docker Desktop or buildx configured with multi-arch builders

Write-Host "Building multi-arch images (amd64, arm64) for ferric-core..." -ForegroundColor Cyan

# Create a new buildx builder instance if needed (or use the default if it supports both)
# To ensure cross-platform building works, we often need to use a docker-container driver.
# docker buildx create --use --name multiarch-builder

# Build for linux/amd64 and linux/arm64
# For local testing, we load the architecture of the current machine.
# To push to a registry, you would use --push instead of --load.
docker buildx build --platform linux/amd64,linux/arm64 -f docker/Dockerfile -t ferric-core:latest .

if ($LASTEXITCODE -eq 0) {
    Write-Host "Successfully built multi-arch image: ferric-core:latest" -ForegroundColor Green
} else {
    Write-Host "Failed to build multi-arch image." -ForegroundColor Red
}
