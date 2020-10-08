FROM rustembedded/cross:armv7-unknown-linux-gnueabihf

RUN dpkg --add-architecture armv7 && \
    apt-get update
RUN dpkg --add-architecture armhf && \
    apt-get update

# RUN apt-get install gcc-arm-linux-gnueabihf
# ignore error in fontconfig install (font cache always fails)
RUN apt install -y fontconfig:armhf || :
RUN apt install -y libfontconfig-dev:armhf || :
# RUN apt install -y fontconfig || :
# RUN apt install -y libfontconfig-dev || :

COPY build_openssl.sh / 
# # first parameter is the openssl arch target (see openssls ./Configure LIST)
# # second parameter is the gcc compiler prefix
RUN bash /build_openssl.sh linux-armv4 arm-linux-gnueabihf-

ENV OPENSSL_DIR=/openssl \
    OPENSSL_INCLUDE_DIR=/openssl/include \
    OPENSSL_LIB_DIR=/openssl/lib \
    OPENSSL_STATIC=1

# RUN apt install -y libssl-dev:armhf libfontconfig1:armhf
# ENV OPENSSL_LIB_DIR=/usr/lib/arm-linux-gnueabihf/
# ENV OPENSSL_INCLUDE_DIR=/usr/include/arm-linux-gnueabihf/openssl
# ENV OPENSSL_STATIC=1

ENV PKG_CONFIG_PATH=/usr/lib/arm-linux-gnueabihf/pkgconfig
