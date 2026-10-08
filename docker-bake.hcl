variable "DEFAULT_TAG" {
  default = "unleashed-voucher-manager:local"
}

// Special target: https://github.com/docker/metadata-action#bake-definition
target "docker-metadata-action" {
  tags = ["${DEFAULT_TAG}"]
}

group "default" {
  targets = ["image-local"]
}

target "image" {
  inherits = ["docker-metadata-action"]
  context  = "."
  target   = "runtime"
  // GHCR links a package to its repository from the manifest annotation,
  // not the config label.
  annotations = [
    "index,manifest:org.opencontainers.image.source=https://github.com/greyrock-labs/unleashed-voucher-manager"
  ]
}

target "image-local" {
  inherits = ["image"]
  output   = ["type=docker"]
}

target "image-all" {
  inherits = ["image"]
  // amd64 only: the Forgejo runner cannot mount binfmt_misc, so QEMU
  // emulation is unavailable there. Do not add setup-qemu-action.
  platforms = [
    "linux/amd64"
  ]
}
