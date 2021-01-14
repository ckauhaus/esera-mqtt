all: target/release/esera target/release/climate

target/release/esera target/release/climate: src/*.rs src/*/*.rs
	cargo build --release
	strip $@

install: all
	rsync -azv target/release/esera target/release/climate root@omv.j.kauhaus.de:
