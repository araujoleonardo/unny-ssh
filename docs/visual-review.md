# Revisão visual — 14/11/2026

A escala de controles está centralizada em `src/ui/controls.css`, importada após
os estilos de layout em `src/main.ts`:

| Contexto | Altura |
| --- | --- |
| Ações compactas e botões de ícone | 28 px |
| Busca e barras de ferramentas | 32 px |
| Campos e ações de formulários | 36 px |

As linhas virtualizadas de arquivos mantêm 30 px, conforme o cálculo da árvore.
Abas de terminal e itens de conexão mantêm suas alturas próprias para acomodar
seu conteúdo.

## Correções

- Botões primários, secundários e campos de formulários usam a mesma altura e
  linha de texto. Rótulos e campos compartilham espaçamentos consistentes.
- Host e porta ficam alinhados em uma grade com colunas que não transbordam.
- Cancelar/confirmar nos formulários de Docker usam a mesma linha de ações dos
  demais diálogos, com alinhamento à direita.
- Buscas de conexões usam uma borda e um indicador de foco no contêiner, sem
  aplicar novamente a borda padrão ao input interno.
- Cabeçalhos, barras laterais, ícones e abas têm alvos e alinhamentos uniformes.
- O botão Salvar na faixa de documentos usa a escala compacta da barra.
- Controles desabilitados usam cursor neutro e contraste reduzido.
- Menus preservam o indicador de foco por teclado; a opção Excluir usa destaque
  contextual, sem um bloco preenchido de largura inteira.
- Diálogos têm largura limitada, padding consistente e rolagem em janelas baixas.
  Prévia de rascunho e inspeção Docker respeitam a largura interna do diálogo.
- Logo, títulos e rodapé do cofre usam proporções menores e alinhadas.
- A animação de entrada do cartão do cofre foi removida para exibir seu conteúdo
  imediatamente, inclusive na captura e inspeção da janela.

## Verificação

Inspeção visual no Firefox instalado, usando os componentes reais com respostas
IPC sintéticas em uma página local temporária (removida após a revisão):

- Cadastro de conexão: alinhamento de inputs, selects, rótulos, porta e botão.
- Workspace e dropdown: busca, ícones, perfis, foco e seleção.
- Árvore, terminal e inventário Docker em 1280×820.
- Cofre e diálogos em 800×600; a navegação por teclado alcança o botão Salvar no
  formulário com rolagem.
- Registro Compose: input e botões Cancelar/Confirmar alinhados.

`npm test`: 37 testes aprovados. `npm run build`: aprovado. Pacote debug macOS
recompilado com `npm run tauri build -- --debug --bundles app`.

Esta revisão confere apresentação e navegação dos componentes. Não substitui
os testes de integração SSH/SFTP/Docker. O resultado visual ainda precisa de
aceite nos WebViews de Linux e Windows.
