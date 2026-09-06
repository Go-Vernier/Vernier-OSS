#!/bin/sh
# Shallow-clones the test corpus into ./corpus (gitignored).
# Real multi-service repositories with genuine service boundaries.
# Every change to blast-radius should run against all of them.
set -e
cd "$(dirname "$0")/.."
mkdir -p corpus
clone() {
  name="$1"; url="$2"
  if [ -d "corpus/$name/.git" ]; then
    echo "  have    $name"
  else
    echo "  clone   $name"
    git clone --quiet --depth 1 "$url" "corpus/$name"
  fi
}
echo "Test corpus:"
clone microservices-demo             https://github.com/GoogleCloudPlatform/microservices-demo.git
clone sock-shop                      https://github.com/microservices-demo/microservices-demo.git
clone robot-shop                     https://github.com/instana/robot-shop.git
clone train-ticket                   https://github.com/FudanSELab/train-ticket.git
clone spring-petclinic-microservices https://github.com/spring-petclinic/spring-petclinic-microservices.git
clone opentelemetry-demo             https://github.com/open-telemetry/opentelemetry-demo.git
clone eshop                          https://github.com/dotnet/eShop.git
clone ewolff-microservice            https://github.com/ewolff/microservice.git
echo "Done. Run: cargo run -- analyze corpus/<name>"
