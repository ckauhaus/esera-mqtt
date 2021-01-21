all: target/release/bridge target/release/climate

target/release/bridge target/release/climate: src/*.rs src/*/*.rs
	cargo build --release
	strip $@

install: all
	rsync -azv target/release/bridge target/release/climate root@omv.j.kauhaus.de:
