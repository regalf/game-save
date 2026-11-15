# Build stage: compile a static binary with the pure Go SQLite driver.
FROM golang:1.27-alpine AS build

WORKDIR /src

# Dependencies first, so the layer is cached across source changes.
COPY go.mod go.sum ./
RUN go mod download

COPY . .

ARG VERSION=dev
# CGO is disabled on purpose: modernc.org/sqlite is a pure Go driver, so the
# result is a fully static binary that runs on any Linux distribution.
RUN CGO_ENABLED=0 GOOS=linux go build \
      -trimpath \
      -ldflags "-s -w -X main.version=${VERSION}" \
      -o /out/gamesave ./cmd/gamesave

# Runtime stage.
FROM alpine:3.20

RUN apk add --no-cache ca-certificates tzdata \
    && adduser -D -u 10001 gamesave

COPY --from=build /out/gamesave /usr/local/bin/gamesave

RUN mkdir -p /data && chown gamesave:gamesave /data
USER gamesave
WORKDIR /data

VOLUME ["/data"]
EXPOSE 8443

ENTRYPOINT ["/usr/local/bin/gamesave"]
CMD ["serve", "--config", "/data/config.yaml"]
