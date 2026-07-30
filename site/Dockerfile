# Static page. Caddy rather than nginx because it takes $PORT without templating.
FROM caddy:2-alpine
COPY Caddyfile /etc/caddy/Caddyfile
COPY index.html /srv/index.html
COPY Masse.zip /srv/Masse.zip
