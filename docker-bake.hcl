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
  // GHCR links a package to its repo from the manifest ANNOTATION, not the
  // config label -- the label alone leaves the package orphaned.
  annotations = [
    "index,manifest:org.opencontainers.image.source=https://github.com/greyrock-labs/unleashed-voucher-manager"
  ]
}

target "image-local" {
  inherits = ["image"]
  output = ["type=docker"]
}

target "image-all" {
  inherits = ["image"]
  // amd64 only, by choice. The Forgejo runner cannot mount binfmt_misc, so
  // QEMU emulation is unavailable there -- do not add setup-qemu-action.
  platforms = [
    "linux/amd64"
  ]
}
