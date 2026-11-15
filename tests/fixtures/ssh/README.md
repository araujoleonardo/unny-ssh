# OpenSSH e Agent nativos descartáveis

```sh
bash tests/fixtures/ssh/run-native.sh
```

Requer `ssh-keygen`, `sshd`, `sftp-server`, `ssh-agent`, `ssh-add`, Bash e Python 3.
No Ubuntu, instale `openssh-server openssh-client`; `/run/sshd` precisa existir
(preparado na CI). macOS usa as ferramentas OpenSSH do sistema.

O script gera chaves temporárias, publica SSH somente em loopback e inicia um
Agent próprio, passado ao processo de teste por `SSH_AUTH_SOCK`. Não modifica
`~/.ssh`, `authorized_keys`, configuração SSH, Agent pessoal ou servidores reais.
O servidor de teste restringe comandos a `/bin/sh -i` e ao binário SFTP conhecido.
`StrictModes no` vale exclusivamente para essa configuração efêmera.

A integração ignorada `services::sessions::tests::native_openssh` é executada pelo
script, que limpa suas chaves, diretórios e processos ao terminar. Sem esse script,
`cargo test` usa o servidor SSH Rust em loopback e ignora a fixture nativa.

Para smoke da UI no macOS, após gerar o bundle debug:

```sh
npm run tauri build -- --debug --bundles app
bash tests/fixtures/ssh/run-native.sh --desktop
```

O modo desktop cria um cofre descartável em `UNNY_SMOKE_DATA_DIR`, com perfil/key
salvos e host de teste confiado; o helper de testes contém a credencial sintética
para conduzir o smoke. Fechar o aplicativo encerra a fixture e remove os dados.
A variável de diretório de smoke é ignorada em release. Não use cofres reais como
fixtures. Não execute o modo desktop com outra instância do unny-ssh aberta.

Para arquivos e transferências reais:

```sh
bash tests/fixtures/ssh/run-native.sh --files
```

Esse modo executa `services::sessions::tests::native_files`: CRUD, permissões,
texto BOM/CRLF, conflitos/recriação após remoção, download de 8 MiB, ZIP com links
ignorados, upload e fila/colisões/cancelamento. O modo desktop inicia no diretório
remoto dessa fixture. Nomes não UTF-8 são criados quando o filesystem permite;
APFS local rejeita esses nomes, e o adaptador tem teste duplex de preservação de
bytes independente do filesystem. Nenhuma ausência desse cenário é contada como
aprovação nativa; consulte as evidências da feature.
