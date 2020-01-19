cross build --target=armv7-unknown-linux-gnueabihf --release --features plotting stable
rsync -vh --progress \
    target/armv7-unknown-linux-gnueabihf/release/dataserver \
    pi@192.168.1.10:/home/pi/dataserver/stable/dataserver \
