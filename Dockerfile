FROM rust:alpine

WORKDIR /srv

COPY . .


RUN cargo install --path .

ENTRYPOINT [ "gravel-gateway" ]

EXPOSE 4278
