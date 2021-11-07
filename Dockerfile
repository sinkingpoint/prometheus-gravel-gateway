FROM ekidd/rust-musl-builder:stable as builder

WORKDIR /usr/src/app

COPY . .

RUN cargo install --path .


FROM alpine:latest as runner

ARG APP=/usr/src/app

ENV TZ=Etc/UTC \
    APP_USER=appuser

RUN addgroup -S $APP_USER \
    && adduser -S -g $APP_USER $APP_USER

RUN apk add --no-cache ca-certificates tzdata

WORKDIR ${APP}

COPY --from=builder ${APP}/target/x86_64-unknown-linux-musl/release/gravel-gateway .

RUN chown -R $APP_USER:$APP_USER ${APP}

USER $APP_USER

EXPOSE 4278

ENTRYPOINT [ "./gravel-gateway", "-l", "0.0.0.0:4278" ]

HEALTHCHECK --interval=30s --timeout=3s \
  CMD wget --spider localhost:4278/metrics
