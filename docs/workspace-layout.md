# Layout do workspace

Atualizado em 14/11/2026 com base nas referências visuais do WebStorm.

Os ícones do workspace usam o pacote `lucide`, com imports individuais em
`src/ui/icons.ts`, traço de 1,5 px e tamanho controlado pela classe `ui-icon`.
A ferramenta Docker usa o ícone `Container`.

A barra superior usa um seletor de conexões com busca, sessões abertas e conexões
salvas. Escolher uma conexão salva reutiliza sua sessão conectada ou em abertura;
o botão **+** ao lado abre outra sessão. O **×** de uma sessão fecha seu contexto,
respeitando a proteção de edições e transferências. **Gerenciar conexões** mostra
os perfis na lateral. Sem sessões, a lateral apresenta as conexões salvas, mesmo
se tiver sido recolhida no uso anterior.

Ao conectar, a lateral passa à árvore de arquivos. Em cada nível, pastas aparecem
antes dos arquivos; os nomes usam ordem alfabética natural, sem distinguir
maiúsculas de minúsculas (`file2` antes de `file10`). Cada página adicional é
integrada à ordenação das entradas já carregadas, preservando pastas expandidas. O botão de conexões no cabeçalho
da árvore permite voltar aos perfis. Abas de documentos ficam no topo do editor;
o terminal ocupa a faixa inferior, incluindo a largura da lateral. Divisórias,
recolhimento e atalhos Cmd/Ctrl+B e Cmd/Ctrl+J continuam disponíveis. No macOS,
a barra superior integra os controles nativos da janela.

Use o botão direito em uma entrada da árvore (ou Shift+F10) para abrir seu menu:

- **Abrir no terminal** em pastas: abre outra aba de terminal da mesma sessão,
  com o diretório selecionado como diretório de trabalho.
- **Abrir no editor** em arquivos; baixar arquivo ou pasta como ZIP.
- Criar arquivo/pasta e enviar arquivos para a pasta selecionada.
- Atualizar, renomear ou excluir, com as confirmações remotas existentes.

Pastas só podem ser excluídas se estiverem vazias. Links não são seguidos.
A abertura no terminal resolve o identificador SFTP no backend e preserva os bytes
do caminho na solicitação SSH, sem simular digitação de comandos no PTY.
Menus aceitam setas, Home/End e Escape e fecham ao clicar fora.

## Validação

- `npm test`: 36 testes aprovados, incluindo seleção, busca e fechamento do
  dropdown, menu de pasta, cancelamento de exclusão e lateral inicial.
- `npm run build`, `cargo check` e `cargo fmt -- --check`: aprovados.
- `cargo test --lib`: 41 aprovados; 11 testes dependentes de fixture ignorados.
- `bash tests/fixtures/ssh/run-native.sh --files`: integração SFTP aprovada.
- `npm run tauri build -- --debug --bundles app`: pacote macOS gerado.
- Inspeção no app macOS com cofre e servidor OpenSSH descartáveis: conexões
  iniciais, dropdown, árvore, arquivo no Monaco e terminal inferior verificados.
  **Abrir no terminal** em `nested` abriu uma segunda aba, e `pwd` retornou o
  diretório remoto terminado em `/remote/nested`.

A inspeção visual foi realizada no macOS. Linux e Windows ainda exigem aceite
visual nativo. A demonstração estática de `npm run dev` continua separada do
workspace SSH nativo; use `npm run tauri dev` para validar esses fluxos.

## Identidade visual

Os arquivos originais ficam em `src/assets/identidade/`:

- `logo.png`: marca horizontal nas telas do cofre e na prévia web.
- `logo-reduzida.png`: marca na tela inicial do workspace.
- `icone.png`: símbolo no seletor sem sessão, favicon e fonte dos ícones desktop.

`src/ui/brand.ts` centraliza a aplicação das imagens. As margens transparentes
são enquadradas por viewports SVG, preservando os PNGs fornecidos e as proporções.
As sessões mantêm suas iniciais coloridas para diferenciar os servidores.

Os arquivos PNG, ICO e ICNS em `src-tauri/icons/` foram gerados com o comando
`tauri icon`, usando `src/assets/identidade/icone.png` como fonte. Ao atualizar a
arte, regenere os ícones desktop e recompile o pacote. Os ícones de ações e
navegação continuam usando Lucide.

A escala de controles e os ajustes de proporção/alinhamento estão descritos na
[revisão visual](visual-review.md).
