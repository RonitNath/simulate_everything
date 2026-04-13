FROM caddy:2-alpine
COPY replays/ /srv
EXPOSE 80
CMD ["caddy", "file-server", "--root", "/srv", "--listen", ":80"]
