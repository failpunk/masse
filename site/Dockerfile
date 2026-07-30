# Static page. Caddy rather than nginx because it takes $PORT without templating.
FROM caddy:2-alpine
COPY Caddyfile /etc/caddy/Caddyfile
COPY index.html /srv/index.html
COPY favicon.ico icon.svg icon-16.png icon-32.png icon-180.png icon-512.png /srv/
COPY Masse.zip /srv/Masse.zip
