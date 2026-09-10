import yaml
import os

CONFIG_FILE = 'synapse_data/mas-config.yaml'

with open(CONFIG_FILE) as f:
    config = yaml.safe_load(f)
    # See https://element-hq.github.io/matrix-authentication-service/setup/homeserver.html
    config['matrix'] = {
        'kind': 'synapse',
        'homeserver': f'{os.environ.get("PODMAN_SERVICE_HOSTNAME")}',
        # 'homeserver': f'{os.environ.get("PODMAN_SERVICE_HOSTNAME")}:{os.environ.get("SYNAPSE_TLS_PORT")}',
        'endpoint': f'https://{os.environ.get("PODMAN_SERVICE_HOSTNAME")}:{os.environ.get("SYNAPSE_TLS_PORT")}',
        'secret': 'superseecret'
    }
    # See https://element-hq.github.io/matrix-authentication-service/setup/database.html
    config['database'] = {
        'host': f'{os.environ.get("PODMAN_SERVICE_HOSTNAME")}',
        'port': int(os.environ.get("MAS_POSTGRES_PORT")),
        'database': f'{os.environ.get("MAS_POSTGRES_DATABASE")}',
        'username': f'{os.environ.get("MAS_POSTGRES_USER")}',
        'password': f'{os.environ.get("MAS_POSTGRES_PASSWORD")}',
        'ssl_mode': 'disable'
    }
    # See https://element-hq.github.io/matrix-authentication-service/setup/sso.html#keycloak
    config['upstream_oauth2'] = {
        'providers': [{
            'id': f'{os.environ.get("MAS_KEYCLOAK_ULID")}',
            'client_id': 'matrix-mas',
            'client_secret': 'mas_secret',
            'issuer': f'https://{os.environ.get("PODMAN_SERVICE_HOSTNAME")}:8443/realms/giz',
            'scope': 'openid profile',
            'token_endpoint_auth_method': 'client_secret_basic'
        }]
    }
    # See https://element-hq.github.io/matrix-authentication-service/reference/configuration.html#http
    config['http'] = {
        'listeners': [{
            'name': 'web',
            'resources': [{
                'name': 'discovery'
            }, {
                'name': 'human'
            }, {
                'name': 'oauth'
            }, {
                'name': 'compat'
            }, {
                'name': 'graphql'
            }, {
                'name': 'assets'
            }, {
                'name': 'adminapi'
            }, {
                'name': 'health'
            }],
            'binds': [{
                'address': '0.0.0.0:8080'
            }],
            'proxy_protocol': False
        }],
        'trusted_proxies': [
            '192.168.0.0/16',
            '172.16.0.0/12',
            '10.0.0.0/10',
            '127.0.0.1/8',
            'fd00::/8',
            '::1/128'
        ],
        'public_base': f'https://{os.environ.get("PODMAN_SERVICE_HOSTNAME")}:{os.environ.get("MAS_TLS_PORT")}/',
        'issuer': f'https://{os.environ.get("PODMAN_SERVICE_HOSTNAME")}:{os.environ.get("MAS_TLS_PORT")}/'
    }
    # See https://element-hq.github.io/matrix-authentication-service/reference/configuration.html#policy
    config['policy'] = {
        'data': {
            'client_registration': {
                'allow_insecure_uris': True
            }
        }
    }

with open(CONFIG_FILE, 'w') as f:
    yaml.dump(config, f, default_flow_style=False, sort_keys=False)
