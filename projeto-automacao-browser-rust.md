# Automação de navegador leve em Rust (CDP) com bindings Python

Guia de projeto do começo ao fim: uma alternativa leve ao Playwright, com núcleo em Rust, controlando um Chromium real via Chrome DevTools Protocol (CDP), com perfis de fingerprint coerentes e bindings para Python.

---

## 1. Visão e escopo

### 1.1 Objetivo

Uma biblioteca que:

- Pesa poucos MB (sem runtime Node.js).
- Controla um Chromium/Chrome/Edge real via CDP.
- Tem API de alto nível (`goto`, `click`, `fill`, `locator`, auto-wait, `screenshot`, `evaluate`, interceptação de rede).
- Aplica perfis de fingerprint coerentes para que o navegador se comporte como um navegador comum de usuário.
- Pode ser usada em Rust (crate) e Python (wheel via PyO3).

### 1.2 Fora de escopo (v1)

- Motor de renderização próprio.
- Firefox e WebKit (futuro: WebDriver BiDi).
- Gravador de testes, trace viewer, runner de testes.

### 1.3 Casos de uso legítimos

QA e testes end-to-end, scraping de dados públicos, monitoramento, pesquisa de privacidade e de detecção de bots, geração de screenshots e PDFs.

### 1.4 Aviso de uso

Contornar detecção de bots para violar termos de serviço, burlar controles de segurança ou cometer fraude não é o propósito do projeto. Documente isso no README e respeite `robots.txt`, limites de taxa e termos dos sites.

### 1.5 Decisões de arquitetura (e por quê)

| Decisão | Escolha | Motivo |
|---|---|---|
| Navegador | Chromium real | TLS, HTTP/2 e comportamento autênticos |
| Protocolo | CDP | Sem servidor intermediário, leve |
| Transporte | Pipe (`--remote-debugging-pipe`) com WebSocket como alternativa | Pipe reduz sinais detectáveis e é mais rápido |
| Núcleo | Rust + tokio | Desempenho, segurança de memória |
| Bindings | PyO3 + maturin, abi3 | Uma wheel por SO/arquitetura |
| API Python | Síncrona por padrão, assíncrona opcional | Maioria dos usuários de automação prefere síncrona |
| Headless | `--headless=new` | Muito mais próximo do Chrome normal |

---

## 2. Estrutura do repositório

```text
meu-browser/
├── Cargo.toml
├── crates/
│   ├── cdp-types/
│   ├── cdp-client/
│   ├── launcher/
│   ├── profiles/
│   ├── core/
│   └── py/
├── python/
│   └── meu_browser/
│       ├── __init__.py
│       └── py.typed
├── tests/
│   ├── integration/
│   └── fingerprint/
├── pyproject.toml
├── scripts/
│   └── gen_cdp.rs
├── protocol/
│   ├── browser_protocol.json
│   └── js_protocol.json
└── .github/workflows/
```

`Cargo.toml` da raiz:

```toml
[workspace]
resolver = "2"
members = [
    "crates/cdp-types",
    "crates/cdp-client",
    "crates/launcher",
    "crates/profiles",
    "crates/core",
    "crates/py",
]

[workspace.dependencies]
tokio = { version = "1", features = ["rt-multi-thread", "macros", "process", "io-util", "sync", "time", "net"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tracing = "0.1"
futures = "0.3"
```

Responsabilidade de cada crate:

- **cdp-types**: structs e enums gerados a partir das especificações JSON do protocolo.
- **cdp-client**: conexão, envio de comandos, correlação de respostas por id, despacho de eventos.
- **launcher**: encontrar/baixar o navegador, montar flags, iniciar o processo, gerenciar perfil temporário.
- **profiles**: perfis de fingerprint coerentes e aplicação via CDP.
- **core**: `Browser`, `Context`, `Page`, `Locator`, auto-wait, rede.
- **py**: bindings PyO3.

---

## 3. Fase 0: Pesquisa e preparação (1 semana)

1. Estudar o código dos projetos existentes para aprender, não para copiar:
   - `chromiumoxide` (cliente CDP em Rust com codegen)
   - `headless_chrome`
   - `nodriver` / `zendriver` (Python, CDP direto)
   - `rebrowser-patches` (documentação dos vazamentos do CDP)
2. Baixar `browser_protocol.json` e `js_protocol.json` do repositório oficial `ChromeDevTools/devtools-protocol`.
3. Ler a documentação do CDP: domínios `Target`, `Page`, `Runtime`, `DOM`, `Input`, `Network`, `Fetch`, `Emulation`.
4. Definir a versão mínima do Chromium suportada.
5. Montar a lista de sites de teste (seção 11).

Entregável: documento de decisões e lista de domínios CDP que o MVP usa.

---

## 4. Fase 1: Cliente CDP (2 a 3 semanas)

### 4.1 Geração de tipos

Escrever um gerador (`scripts/gen_cdp.rs`) que leia os JSONs do protocolo e produza Rust:

- Um módulo por domínio (`page`, `network`, `runtime`...).
- Um struct por comando (parâmetros) e por retorno.
- Um enum por evento.
- `#[serde(rename_all = "camelCase")]` e `Option` para campos opcionais.
- Tipos marcados como `experimental` ou `deprecated` atrás de feature flags.

Ponto de atenção: manter o gerador reexecutável para acompanhar novas versões do protocolo.

### 4.2 Transporte

Duas implementações atrás de um trait:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, message: String) -> Result<(), TransportError>;
    async fn recv(&self) -> Result<Option<String>, TransportError>;
}
```

- **PipeTransport**: o Chrome com `--remote-debugging-pipe` usa os file descriptors 3 e 4; as mensagens são JSON terminadas em byte nulo (`\0`). Em Rust, criar os pipes e passá-los ao processo filho (no Unix com `pre_exec` e `dup2`; no Windows com handles herdados).
- **WebSocketTransport**: `tokio-tungstenite`, conectando na URL de `/json/version` com `--remote-debugging-port`.

### 4.3 Conexão e despacho

- Contador atômico de ids de comando.
- Mapa `id -> oneshot::Sender` para as respostas.
- Canal de broadcast para eventos, com filtro por `sessionId` e por método.
- Timeout configurável por comando.
- Tratamento de desconexão: todas as requisições pendentes falham com erro claro.

```rust
pub struct Connection {
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, CdpError>>>>,
    events: broadcast::Sender<Event>,
    transport: Arc<dyn Transport>,
}

impl Connection {
    pub async fn call<C: Command>(
        &self,
        session: Option<&SessionId>,
        cmd: C,
    ) -> Result<C::Response, CdpError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let payload = serialize_command(id, session, &cmd)?;
        self.transport.send(payload).await?;
        let value = tokio::time::timeout(self.timeout, rx)
            .await
            .map_err(|_| CdpError::Timeout)??;
        Ok(serde_json::from_value(value)?)
    }
}
```

### 4.4 Sessões e targets

- Usar `Target.setAutoAttach` com `flatten: true` para tratar todas as sessões na mesma conexão, diferenciadas por `sessionId`.
- Rastrear targets (páginas, iframes, workers, service workers).
- Usar `waitForDebuggerOnStart` para aplicar overrides antes de qualquer script da página rodar, e então chamar `Runtime.runIfWaitingForDebugger`.

### 4.5 Critério de pronto

- Abrir o Chrome, navegar para uma URL, receber `Page.loadEventFired` e fechar limpo, em Linux, macOS e Windows.
- Testes de unidade com transporte falso.

---

## 5. Fase 2: Launcher (1 a 2 semanas)

### 5.1 Localizar o navegador

Ordem de busca:

1. Caminho explícito do usuário.
2. Variável de ambiente (`MEU_BROWSER_EXECUTABLE`).
3. Caminhos padrão por SO (Chrome, Chromium, Edge, Brave).
4. Download automático do "Chrome for Testing" (JSON de versões publicado pelo Google), com verificação de checksum e cache em diretório de dados do usuário.

### 5.2 Flags base

```text
--headless=new
--remote-debugging-pipe
--user-data-dir=<diretório temporário>
--no-first-run
--no-default-browser-check
--disable-background-networking
--disable-component-update
--disable-sync
--metrics-recording-only
--password-store=basic
--use-mock-keychain
```

Evitar flags que denunciam automação ou que não existem em um uso normal; cada flag extra altera o fingerprint. Revisar a lista a cada versão.

### 5.3 Ciclo de vida

- Perfil temporário criado e removido ao fechar (opção de perfil persistente).
- Encerramento limpo: `Browser.close`, aguardar o processo, e `kill` após timeout.
- Garantir que processos órfãos não fiquem para trás (usar `kill_on_drop` e, no Linux, `prctl(PR_SET_PDEATHSIG)`).
- Suporte a rodar sem sandbox em containers (`--no-sandbox` apenas quando necessário e documentado).
- Proxy configurável (`--proxy-server`), com autenticação tratada via `Fetch.authRequired`.

### 5.4 Critério de pronto

`Browser::launch(options).await` funciona em CI nos três sistemas, e o processo some quando o programa termina, inclusive em caso de panic.

---

## 6. Fase 3: API de alto nível (4 a 6 semanas)

### 6.1 Modelo de objetos

```text
Browser
 └── BrowserContext   (isolamento: cookies, storage, permissões)
      └── Page
           ├── Frame
           └── Locator
```

Contextos isolados usam `Target.createBrowserContext`; cada um com seu próprio perfil de fingerprint opcional.

### 6.2 Navegação

- `goto(url, wait_until)` com estados: `commit`, `domcontentloaded`, `load`, `networkidle`.
- `networkidle` exige rastrear requisições em andamento via eventos de `Network`.
- `reload`, `go_back`, `go_forward`, `wait_for_url`.

### 6.3 Execução de JavaScript

- `evaluate(expression)` e `evaluate_handle`.
- Retorno por valor ou por handle (`RemoteObject`), com serialização cuidadosa de tipos especiais (`undefined`, `NaN`, `BigInt`, `Date`).
- Usar mundos isolados (`Page.createIsolatedWorld`) para o código da própria biblioteca, de forma que scripts da página não enxerguem nem interfiram nos seus helpers.
- Evitar chamar `Runtime.enable` no mundo principal sempre que possível, pois é um vazamento conhecido; obter contextos por `Page.createIsolatedWorld` e `Runtime.evaluate` com `contextId` explícito.

### 6.4 Locators e seletores

Implementar um motor de seletores injetado em mundo isolado (JavaScript embutido no binário via `include_str!`):

- CSS e XPath.
- `text=`, `role=` (com nome acessível), `label=`, `placeholder=`, `test-id=`.
- Encadeamento (`locator.locator(...)`), `nth`, `first`, `last`, `filter`.
- Travessia de shadow DOM aberto.
- Frames aninhados.

Locators são preguiçosos: guardam a consulta, resolvem a cada ação (como no Playwright), o que evita referências obsoletas.

### 6.5 Auto-wait e actionability

Antes de `click`, `fill`, `type` e similares, verificar em laço com timeout:

1. Elemento existe e é único (ou modo estrito configurável).
2. Está visível (tem caixa de layout, `visibility` não oculta).
3. Está estável (posição igual em dois frames de animação consecutivos).
4. Recebe eventos (o ponto de clique não está coberto por outro elemento, verificado com `elementFromPoint`).
5. Está habilitado (e editável, para `fill`).

Se falhar, esperar e tentar de novo com backoff curto; no timeout, retornar erro com o motivo específico (por exemplo, "coberto por `<div class=overlay>`").

### 6.6 Entrada (Input)

- `click`: rolar até o elemento (`DOM.scrollIntoViewIfNeeded`), calcular o centro, `Input.dispatchMouseEvent` (`mouseMoved`, `mousePressed`, `mouseReleased`).
- `type`: `Input.dispatchKeyEvent` por tecla, com atrasos configuráveis.
- `fill`: focar, selecionar tudo e `Input.insertText`, ou setar valor e disparar eventos `input`/`change`.
- `press`, `hover`, `drag_and_drop`, `select_option`, `check`/`uncheck`, `set_input_files` (`DOM.setFileInputFiles`).
- Movimento de mouse e digitação com temporização configurável (útil em testes de UX e para parecer humano).

### 6.7 Rede

- Eventos de requisição/resposta (`Network.*`).
- Interceptação com `Fetch.enable` e padrões: `route(pattern, handler)` com `continue`, `fulfill`, `abort`.
- Leitura de corpo de resposta (`Network.getResponseBody`).
- Cookies (`Network.getCookies`, `setCookie`) e storage state exportável/importável em JSON.
- Cabeçalhos extras por contexto.

### 6.8 Outras funcionalidades

- `screenshot` (página inteira, elemento, clip) e `pdf`.
- Diálogos (`Page.javascriptDialogOpening`), downloads (`Browser.setDownloadBehavior`), novas abas/popups (eventos de `Target`).
- Emulação de dispositivo, geolocalização, permissões, tema escuro.
- Tratamento de iframes (frame tree via `Page.getFrameTree` e eventos de attach/detach).

### 6.9 Erros

Tipos de erro claros: `TimeoutError`, `NavigationError`, `ElementNotFound`, `NotActionable { reason }`, `ProtocolError { code, message }`, `BrowserCrashed`. Mensagens devem dizer o que a biblioteca estava esperando e por quanto tempo.

### 6.10 Critério de pronto

Um script de exemplo que: abre uma página, preenche e envia um formulário, espera navegação, intercepta uma requisição, tira screenshot e extrai dados, rodando de forma estável 100 vezes seguidas em CI.

---

## 7. Fase 4: Perfis de fingerprint e anti-detecção (5 a 8 semanas)

### 7.1 Princípio central

**Coerência importa mais que aleatoriedade.** Um perfil é um conjunto completo e internamente consistente de valores, baseado em dados reais de máquinas reais (SO, versão do navegador, GPU, resolução, fontes, idiomas, fuso). Valores soltos e contraditórios são a principal fonte de detecção.

### 7.2 Estrutura de um perfil

```rust
pub struct Profile {
    pub user_agent: String,
    pub user_agent_metadata: UserAgentMetadata,
    pub platform: String,
    pub languages: Vec<String>,
    pub locale: String,
    pub timezone: String,
    pub screen: ScreenInfo,
    pub viewport: Viewport,
    pub device_scale_factor: f64,
    pub hardware_concurrency: u32,
    pub device_memory: u32,
    pub webgl: WebGlInfo,
    pub fonts: Vec<String>,
    pub geolocation: Option<Geolocation>,
}
```

Perfis ficam em arquivos JSON versionados, derivados de coletas reais (coletar com consentimento de máquinas próprias), com validação de consistência na carga (por exemplo, user-agent de Windows exige plataforma `Win32`, fontes e GPU compatíveis com Windows).

### 7.3 Ordem de preferência para aplicar cada propriedade

1. **Flags do navegador e configuração nativa**: `--lang`, `--window-size`, `--force-device-scale-factor`, variável `TZ`.
2. **Comandos CDP nativos** (sem rastro em JavaScript):
   - `Emulation.setUserAgentOverride` (inclui `userAgentMetadata` para Client Hints)
   - `Emulation.setTimezoneOverride`
   - `Emulation.setLocaleOverride`
   - `Emulation.setDeviceMetricsOverride`
   - `Emulation.setGeolocationOverride`
   - `Emulation.setHardwareConcurrencyOverride`
   - `Network.setExtraHTTPHeaders` (com cuidado: afeta a ordem dos cabeçalhos)
3. **Injeção de JavaScript** (`Page.addScriptToEvaluateOnNewDocument`) apenas para o que não tem alternativa, como partes de `navigator` e WebGL.

### 7.4 Injeção de JavaScript sem deixar rastros óbvios

Pontos que detectores verificam:

- `Function.prototype.toString` de funções sobrescritas deve retornar `function name() { [native code] }`.
- Descritores de propriedade (`Object.getOwnPropertyDescriptor`) devem ter o mesmo formato do original (getters no protótipo, não propriedades próprias na instância).
- Nomes, `length` e comportamento de erro das funções devem bater.
- A ordem de chaves e o `prototype` devem ser preservados.
- O script deve rodar em todos os frames e workers (`Target.setAutoAttach` com `waitForDebuggerOnStart`), pois um valor diferente em um iframe ou worker entrega a inconsistência.
- Sobrescrever em workers também (`navigator.hardwareConcurrency` e user-agent em `Worker` precisam coincidir).

Técnica: criar proxies cujo `toString` seja registrado em um `WeakMap` e patchear `Function.prototype.toString` uma única vez, de forma que o próprio patch seja indetectável.

### 7.5 Itens específicos

- **Canvas, WebGL e áudio**: preferir não randomizar a saída por ruído (ruído é detectável e cria fingerprint único a cada execução). Preferir que o ambiente (GPU real ou software renderer coerente com o perfil) produza o resultado. Se for necessário alterar, usar ruído determinístico por perfil.
- **WebGL vendor/renderer**: precisa ser coerente com SO e perfil (`UNMASKED_VENDOR_WEBGL`, `UNMASKED_RENDERER_WEBGL`) e com os parâmetros e extensões suportados.
- **Fontes**: a lista exposta deve corresponder ao SO declarado; em containers Linux declarando Windows, instalar fontes compatíveis ou limitar a enumeração.
- **WebRTC**: evitar vazamento de IP local (`--force-webrtc-ip-handling-policy=disable_non_proxied_udp`) coerente com o uso de proxy.
- **Client Hints**: `Sec-CH-UA*` e `navigator.userAgentData` devem concordar com o user-agent.
- **Idiomas e `Accept-Language`**: coerentes entre si e com o fuso e a localização do proxy.
- **Sinais de automação**: `navigator.webdriver` deve ser `false`/ausente, como em navegador normal (a flag `--disable-blink-features=AutomationControlled` resolve nativamente); sem `window.cdc_*` ou propriedades residuais.
- **Plugins, `mimeTypes`, `permissions`, `chrome.runtime`** no formato do Chrome real.

### 7.6 Rede e TLS

Como o navegador é real, JA3/JA4 e HTTP/2 já são autênticos **desde que a versão do Chrome usada coincida com a declarada no user-agent**. Regra: o perfil deve sempre declarar a versão realmente instalada. Se o launcher baixa o Chrome for Testing, gerar o user-agent a partir da versão real, não de uma lista fixa.

### 7.7 Comportamento humano (opcional)

- Trajetórias de mouse com curvas e jitter, atrasos variáveis de digitação, rolagem em passos, pausas.
- Configurável e desligado por padrão: útil em testes de UX e em casos em que o site mede interação.

### 7.8 Validação automática

Testes que abrem os sites da seção 11, extraem os resultados (via JSON quando disponível ou lendo o DOM) e falham se houver regressão: por exemplo, "CreepJS não reporta `lies`", "`webdriver` é false", "JA4 corresponde ao Chrome da versão X". Guardar os relatórios como artefatos de CI.

### 7.9 Mapa das camadas de detecção

Detectores combinam sinais de várias camadas. Cada técnica abaixo pertence a uma camada; o perfil só é convincente quando todas concordam entre si.

| Camada | O que é observado | Onde tratar |
|---|---|---|
| IP e rede | Reputação do IP, ASN (residencial, datacenter, móvel), geolocalização, DNS | Proxy (7.12) |
| TLS | JA3, JA4, extensões, GREASE, ALPN | Chrome real (7.6) ou cliente impersonador (7.11) |
| HTTP/2 e HTTP/3 | Frame `SETTINGS`, ordem dos pseudo-headers, prioridades, ordem e caixa dos cabeçalhos | Chrome real; cuidado com `setExtraHTTPHeaders` |
| Protocolo de automação | Efeitos colaterais do CDP visíveis em JavaScript | 7.10 |
| Runtime JS | `navigator`, protótipos, `toString`, stack traces, workers | 7.4 |
| Renderização | Canvas, WebGL, WebGPU, áudio, fontes, `getClientRects`, vozes de fala | 7.13 |
| Comportamento | Trajetória do mouse, ritmo de digitação, rolagem, tempo entre ações | 7.14 |
| Sessão e histórico | Idade do perfil, cookies, referrer, concorrência, taxa de requisições | 7.15 |

### 7.10 Vazamentos do CDP e como evitá-los

Esses são os sinais que distinguem um Chrome controlado por CDP de um Chrome comum, mesmo com todo o fingerprint correto.

1. **`Runtime.enable` no mundo principal.** Ao habilitar o domínio, o Chrome passa a serializar objetos logados no console, o que permite a uma página detectar o debugger por efeitos colaterais (por exemplo, um `Error` com getter em `stack` sendo tocado ao ser passado para `console.debug`). Mitigação: não chamar `Runtime.enable` no mundo principal; criar mundos com `Page.createIsolatedWorld` e avaliar com `Runtime.evaluate` informando `contextId`/`uniqueContextId` explícito; rastrear contextos por `Page.frameNavigated` e criação de mundos, não por `Runtime.executionContextCreated`.
2. **Marcadores de origem de script.** Evitar `//# sourceURL=` com nomes característicos da biblioteca em qualquer código avaliado; nomear mundos isolados com valores neutros.
3. **Eventos de mouse via `Input.dispatchMouseEvent`.** Verificar que `screenX`/`screenY` dos eventos resultantes sejam coerentes com a posição real da janela e não idênticos a `clientX`/`clientY`. O detector da Rebrowser e o CreepJS testam isso; calcular o deslocamento da janela e compensar nas coordenadas.
4. **`Emulation.setDeviceMetricsOverride`.** Cria divergências entre `window.innerWidth`, `outerWidth`, `screen.width` e `visualViewport`. Preferir `--window-size`, `--force-device-scale-factor` e janela real; usar o override só quando necessário e então ajustar todos os valores relacionados.
5. **`Fetch.enable` permanente.** Altera cache, ordem de eventos e temporização. Habilitar apenas com padrões específicos e desabilitar quando não houver `route` ativo.
6. **`Page.setBypassCSP`, `Network.setCacheDisabled`, `Emulation.setScriptExecutionDisabled`.** Têm efeitos observáveis; evitar por padrão.
7. **Sinais de headless.** Com `--headless=new` a maior parte some, mas conferir: `HeadlessChrome` no user-agent e em `Sec-CH-UA`, `navigator.plugins` e `mimeTypes` vazios, `window.chrome` ausente, `Notification.permission` incoerente com `permissions.query`, `outerHeight` igual a `innerHeight`, ausência de `chrome.runtime`.
8. **Flags de automação.** Nunca passar `--enable-automation`; usar `--disable-blink-features=AutomationControlled` para `navigator.webdriver`; não deixar a barra de infobar aparecer em modo com janela.
9. **Tempo de anexação.** Anexar com `waitForDebuggerOnStart` e aplicar overrides antes de `Runtime.runIfWaitingForDebugger`, em páginas, iframes, workers dedicados, shared workers e service workers.

Alternativa estrutural: controlar o navegador por extensão com native messaging em vez de CDP. A superfície de detecção é diferente (sem os efeitos acima, mas com API limitada e sinais de extensão). Vale como backend opcional futuro, atrás do mesmo trait de transporte.

### 7.11 Camada de rede sem navegador (cliente HTTP impersonador)

Para chamadas em que renderizar a página é desnecessário, oferecer um modo HTTP leve que reutilize o mesmo perfil:

- Biblioteca Rust: `rquest` (BoringSSL com perfis de navegador), ou `curl-impersonate` via FFI.
- Replicar exatamente: ordem e valores das extensões TLS, ALPN, `SETTINGS` do HTTP/2, janela inicial, ordem de pseudo-headers (`:method`, `:authority`, `:scheme`, `:path`) e a ordem e a caixa dos cabeçalhos HTTP.
- Chrome randomiza a ordem das extensões TLS desde a versão 110, então JA3 varia entre conexões legítimas; comparar por JA4 e por conjuntos, não por hash fixo.
- Compartilhar cookies e armazenamento entre o navegador e o cliente HTTP (exportação de `storage state`), mantendo o mesmo user-agent, o mesmo IP de saída e o mesmo perfil TLS, pois cookies de sessão de provedores anti-bot costumam ser amarrados a esses três.

### 7.12 Proxy, IP e coerência geográfica

- Tipos: residencial, móvel e ISP têm reputação melhor que datacenter. Preferir sessões fixas (sticky) por contexto em vez de trocar IP a cada requisição.
- Derivar `timezone`, `locale`, `Accept-Language` e geolocalização do IP de saída (consulta GeoIP no momento da criação do contexto).
- WebRTC: `--force-webrtc-ip-handling-policy=disable_non_proxied_udp` e conferir vazamento em `browserleaks.com/webrtc`.
- DNS: garantir resolução pelo proxy (SOCKS5 com DNS remoto, ou HTTP CONNECT), sem vazar o resolvedor local.
- Autenticação do proxy via `Fetch.authRequired`, restrita à sessão do contexto.
- Um proxy e um perfil por `BrowserContext`; nunca reutilizar o mesmo perfil com IPs de países diferentes.

### 7.13 Fingerprint de renderização e hardware

Princípio: valores reais e estáveis por perfil, nunca ruído novo a cada execução.

- **Canvas, WebGL, áudio, `getClientRects`**: a saída deve vir de um renderizador coerente com o perfil (GPU real, ou SwiftShader/ANGLE declarado como tal). Se houver perturbação, usar semente determinística por perfil, aplicada de forma que a mesma operação sempre devolva o mesmo resultado dentro do perfil.
- **WebGL**: `UNMASKED_VENDOR_WEBGL`, `UNMASKED_RENDERER_WEBGL`, `MAX_TEXTURE_SIZE`, extensões suportadas, precisão de shaders e `getParameter` completos, todos tirados de uma máquina real do mesmo modelo de GPU. Cobrir também `WebGL2` e `WebGPU` (`navigator.gpu.requestAdapter`).
- **Fontes**: lista, métricas de largura e fallback devem corresponder ao SO declarado. Em Linux declarando Windows, instalar um conjunto compatível de fontes e conferir medições de texto.
- **Dispositivos de mídia**: `enumerateDevices` com contagem plausível; `speechSynthesis.getVoices()` coerente com SO e idioma.
- **Tela**: `screen.width/height`, `availWidth/availHeight`, `colorDepth`, `devicePixelRatio`, `matchMedia` (`prefers-color-scheme`, `prefers-reduced-motion`, `pointer`, `hover`).
- **Rede e bateria**: `navigator.connection` (`effectiveType`, `rtt`, `downlink`) plausível; `getBattery` ausente ou coerente com desktop/laptop.
- **Temporização**: `performance.now()` com a resolução do Chrome real; não desacelerar nem acelerar relógios.
- **Internacionalização**: `Intl.DateTimeFormat().resolvedOptions()`, `Intl.NumberFormat`, formatação de datas e `Date.getTimezoneOffset` devem concordar com `timezone` e `locale`.
- **Pool de perfis**: ponderar a seleção pela participação real de mercado (SO, versão do Chrome, GPU, resolução), pois um perfil raro demais também é sinal.

Extensão do `Profile`:

```rust
pub struct NetworkProfile {
    pub accept_language: String,
    pub header_order: Vec<String>,
    pub send_client_hints: bool,
    pub webrtc_policy: WebRtcPolicy,
}

pub struct RenderProfile {
    pub canvas_seed: u64,
    pub audio_seed: u64,
    pub rects_seed: u64,
    pub webgl_params: WebGlParams,
    pub media_devices: MediaDevices,
    pub speech_voices: Vec<String>,
    pub battery: Option<BatteryInfo>,
    pub connection: ConnectionInfo,
}

pub struct Profile {
    pub base: BaseProfile,
    pub network: NetworkProfile,
    pub render: RenderProfile,
    pub behavior: BehaviorConfig,
}
```

### 7.14 Comportamento humano

Desligado por padrão, configurável por contexto.

- **Mouse**: trajetória em curva de Bézier cúbica com pontos de controle perpendiculares ao segmento, perfil de velocidade suave (aceleração e desaceleração), duração proporcional à distância (lei de Fitts), pequeno overshoot com correção, jitter de subpixel, eventos `mousemove` intermediários antes de `mousedown`, e ponto de clique com desvio aleatório dentro da caixa do elemento (não o centro exato).
- **Teclado**: intervalos entre teclas em distribuição log-normal, pausas maiores entre palavras, `keydown`/`keyup` com sobreposição ocasional, erros de digitação e correção opcionais.
- **Rolagem**: eventos `wheel` em passos com inércia, em vez de `scrollTo` instantâneo; pausas de leitura.
- **Foco e visibilidade**: eventos `focus`/`blur` e `visibilitychange` coerentes; evitar abas em segundo plano executando ações.
- **Ritmo**: tempo de reflexão entre ações, jitter nos atrasos, limite de concorrência por domínio e por IP.

```rust
use rand::Rng;
use std::time::Duration;

#[derive(Clone, Copy)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub fn mouse_path<R: Rng>(from: Point, to: Point, rng: &mut R) -> Vec<(Point, Duration)> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let dist = (dx * dx + dy * dy).sqrt().max(1.0);
    let nx = -dy / dist;
    let ny = dx / dist;
    let bend = rng.gen_range(-0.25..0.25) * dist;
    let c1 = Point { x: from.x + dx * 0.3 + nx * bend, y: from.y + dy * 0.3 + ny * bend };
    let c2 = Point { x: from.x + dx * 0.7 + nx * bend * 0.6, y: from.y + dy * 0.7 + ny * bend * 0.6 };
    let steps = ((dist / 8.0) as usize).clamp(12, 120);
    let total_ms = 120.0 + 180.0 * (dist / 100.0 + 1.0).log2();
    let mut out = Vec::with_capacity(steps);
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let e = t * t * (3.0 - 2.0 * t);
        let u = 1.0 - e;
        let x = u.powi(3) * from.x + 3.0 * u * u * e * c1.x + 3.0 * u * e * e * c2.x + e.powi(3) * to.x;
        let y = u.powi(3) * from.y + 3.0 * u * u * e * c1.y + 3.0 * u * e * e * c2.y + e.powi(3) * to.y;
        let p = Point { x: x + rng.gen_range(-0.6..0.6), y: y + rng.gen_range(-0.6..0.6) };
        let delay = total_ms / steps as f64 * rng.gen_range(0.7..1.3);
        out.push((p, Duration::from_secs_f64(delay / 1000.0)));
    }
    out
}
```

### 7.15 Sessão, histórico e taxa

- **Perfil persistente e "envelhecido"**: manter cookies, `localStorage`, histórico e cache entre execuções para o mesmo perfil, em vez de um perfil virgem a cada vez.
- **Navegação natural**: chegar por referrer plausível (busca ou página inicial) em vez de sempre abrir a URL profunda diretamente; carregar recursos estáticos normalmente (não bloquear imagens, CSS e fontes sem necessidade, pois o padrão de carregamento é um sinal).
- **Taxa e concorrência**: limites por domínio, backoff exponencial com jitter em respostas 429/403, reaproveitamento de conexão.
- **Isolamento**: um contexto por identidade; cookies, perfil, IP e horário de atividade consistentes entre si.
- **Detecção de bloqueio**: reconhecer páginas de desafio e respostas anômalas e parar ou reduzir a taxa em vez de insistir.

### 7.16 Navegador próprio versus CDP sobre Chromium

| Abordagem | Vantagens | Custos |
|---|---|---|
| CDP sobre Chromium oficial (escolha atual) | TLS e HTTP/2 autênticos, atualização simples, leve | Superfície de CDP (7.10), fingerprint depende de overrides |
| Chromium compilado com patches (ex.: remover marcadores de automação, fixar valores nativos em C++) | Sem rastros em JS, valores nativos | Compilar o Chromium (horas e dezenas de GB), rebase a cada versão, peso do binário |
| Firefox patcheado (abordagem do Camoufox) | Valores alterados em C++, sem `Function.toString` adulterado | Base de usuários menor, outro conjunto de sinais, manutenção |
| Extensão com native messaging | Sem CDP | API limitada, sinais de extensão |

Recomendação: manter CDP sobre Chromium oficial no v1; documentar o trait `Backend` para permitir um Chromium patcheado como segundo backend sem mudar a API pública.

### 7.17 Fora de escopo, por decisão do projeto

Seguindo as seções 1.4 e 13: resolução de CAPTCHA, falsificação de tokens ou "sensor data" de provedores específicos de anti-bot e engenharia reversa de scripts de desafio não entram na biblioteca. O objetivo permanece sendo um navegador automatizado coerente, não um conjunto de contramedidas por fornecedor.

### 7.18 Testes de regressão adicionais

Além da seção 7.8, gerar um relatório de comparação (navegador automatizado versus Chrome comum, mesma máquina) para:

- Vazamentos de CDP: `bot-detector.rebrowser.net`.
- Sinais de headless e automação: `bot.sannysoft.com`, `arh.antoinevastel.com/bots/areyouheadless`, `deviceandbrowserinfo.com/are_you_a_bot`.
- Consistência entre sinais: `pixelscan.net`, `browserscan.net`, CreepJS.
- TLS e HTTP/2: `tls.peet.ws`, comparando JA4 com o do Chrome da mesma versão.
- Workers: página de teste própria que compara `navigator.*` na página, em iframe, em `Worker`, `SharedWorker` e `ServiceWorker`.
- Estabilidade do perfil: mesma semente, mesmo hash de canvas, WebGL e áudio em 100 execuções.

---

## 8. Fase 5: Bindings Python (2 a 3 semanas)

### 8.1 Configuração

`crates/py/Cargo.toml`:

```toml
[package]
name = "meu-browser-py"
version = "0.1.0"
edition = "2021"

[lib]
name = "_meu_browser"
crate-type = ["cdylib"]

[dependencies]
pyo3 = { version = "0.22", features = ["extension-module", "abi3-py39"] }
pyo3-async-runtimes = { version = "0.22", features = ["tokio-runtime"] }
meu-browser-core = { path = "../core" }
tokio = { workspace = true }
```

`pyproject.toml`:

```toml
[build-system]
requires = ["maturin>=1.5,<2"]
build-backend = "maturin"

[project]
name = "meu-browser"
requires-python = ">=3.9"
dynamic = ["version"]

[tool.maturin]
manifest-path = "crates/py/Cargo.toml"
python-source = "python"
module-name = "meu_browser._meu_browser"
features = ["pyo3/extension-module"]
```

### 8.2 API síncrona

- Um runtime tokio global criado preguiçosamente.
- Cada método chama `runtime.block_on(...)` **liberando o GIL** com `py.allow_threads(...)`, para que outras threads Python rodem durante a espera.
- Trate `KeyboardInterrupt` (checar sinais periodicamente em esperas longas).

```rust
#[pymethods]
impl PyPage {
    fn goto(&self, py: Python<'_>, url: String) -> PyResult<()> {
        let page = self.inner.clone();
        py.allow_threads(|| RUNTIME.block_on(page.goto(&url)))
            .map_err(to_py_err)
    }
}
```

### 8.3 API assíncrona

Classe paralela (`AsyncPage`, etc.) usando `pyo3_async_runtimes::tokio::future_into_py`, compatível com `asyncio`. Manter paridade de métodos entre as duas APIs; considerar gerar uma a partir da outra com macros.

### 8.4 Ergonomia

- Context managers: `with Browser.launch() as browser:` garantindo fechamento.
- Exceções Python mapeadas dos erros Rust (`TimeoutError`, `ElementNotFound`...).
- Type stubs (`.pyi`) e `py.typed` para autocompletar.
- Documentação com exemplos equivalentes aos do Playwright para facilitar a migração.

Exemplo de uso final:

```python
from meu_browser import Browser, Profile

with Browser.launch(profile=Profile.windows_chrome()) as browser:
    page = browser.new_page()
    page.goto("https://example.com")
    page.locator("a").first.click()
    print(page.title())
    page.screenshot(path="out.png")
```

### 8.5 Distribuição

- Wheels `abi3` para: `manylinux_2_28` (x86_64, aarch64), `musllinux`, macOS (x86_64, arm64), Windows (x86_64).
- Publicação no PyPI via GitHub Actions com `maturin-action` e trusted publishing.

---

## 9. Fase 6: Testes (contínuo)

### 9.1 Camadas

1. **Unitários**: serialização CDP, parser de seletores, validação de perfis, lógica de actionability com DOM simulado.
2. **Integração**: Chrome real contra um servidor HTTP local de páginas de teste (formulários, iframes, shadow DOM, popups, downloads, overlays, páginas lentas).
3. **Fingerprint**: seção 11, rodando em CI com relatórios.
4. **Estabilidade (soak)**: loops longos abrindo e fechando milhares de páginas para achar vazamentos de memória e processos órfãos.
5. **Matriz**: SO × versões do Chrome (estável, beta, versão mínima suportada).

### 9.2 Flakiness

Toda falha intermitente é bug de auto-wait ou de sincronização. Registrar logs do protocolo (`tracing`) com opção de dump completo das mensagens CDP para depuração.

---

## 10. Fase 7: Documentação, CI/CD e lançamento

### 10.1 Documentação

- README com instalação, exemplo mínimo, aviso de uso.
- Guia "Migrando do Playwright".
- Referência de API (rustdoc e Sphinx/mkdocs para Python).
- Página sobre perfis: o que cada um altera, limitações, como criar o seu.
- Página de solução de problemas (Chrome não encontrado, sandbox em containers, fontes ausentes).

### 10.2 CI

- `cargo fmt`, `clippy -D warnings`, `cargo test`, `cargo deny` e `cargo audit`.
- Testes Python com `pytest`.
- Build de wheels em matriz.
- Cache de download do Chrome for Testing.

### 10.3 Versionamento e lançamento

- SemVer, `CHANGELOG`.
- Publicar crates no crates.io e wheels no PyPI com a mesma versão.
- Pré-lançamento (`0.x`) com API instável declarada.

### 10.4 Manutenção do protocolo

- Rotina mensal: baixar novo JSON do protocolo, regenerar tipos, rodar a suíte.
- Rotina a cada versão estável do Chrome: atualizar perfis, rodar testes de fingerprint, ajustar o que quebrar.

---

## 11. Sites para testar

| Site | Para que serve |
|---|---|
| `abrahamjuliot.github.io/creepjs` | Inconsistências e "lies" em APIs do navegador |
| `browserleaks.com` | Canvas, WebGL, fontes, WebRTC, TLS, JavaScript |
| `coveryourtracks.eff.org` | Unicidade do fingerprint |
| `amiunique.org` | Comparação com base de fingerprints |
| `bot.sannysoft.com` | Sinais clássicos de automação |
| `pixelscan.net` | Consistência entre sinais |
| `tls.peet.ws` | JA3/JA4 e HTTP/2 |
| `bot-detector.rebrowser.net` | Vazamentos específicos do CDP |
| `browserscan.net` | Consistência e sinais de bot |
| `deviceandbrowserinfo.com/are_you_a_bot` | Testes de automação e de comportamento |
| `arh.antoinevastel.com/bots/areyouheadless` | Sinais de headless |

Como usar: rodar o seu navegador automatizado e um Chrome comum, na mesma máquina, e comparar os relatórios. O objetivo é que não haja diferenças significativas entre os dois.

---

## 12. Cronograma sugerido

| Fase | Duração | Entregável |
|---|---|---|
| 0. Pesquisa | 1 semana | Decisões e escopo |
| 1. Cliente CDP | 2 a 3 semanas | Conexão estável, tipos gerados |
| 2. Launcher | 1 a 2 semanas | Iniciar e fechar o navegador em 3 SOs |
| 3. API alta | 4 a 6 semanas | `Page`, `Locator`, auto-wait, rede |
| 4. Perfis e anti-detecção | 5 a 8 semanas | Perfis coerentes, mitigação de vazamentos CDP, comportamento, testes de fingerprint |
| 5. Python | 2 a 3 semanas | Wheels sync e async |
| 6. Testes | contínuo | Suíte completa em CI |
| 7. Lançamento | 1 a 2 semanas | Docs, PyPI, crates.io |

Total estimado: 4 a 6 meses para uma pessoa, versão 0.1 utilizável.

### MVP (mais ou menos 4 a 6 semanas)

Launcher + cliente CDP + `goto`, `click`, `fill`, `text_content`, `screenshot`, `evaluate`, auto-wait simples, um perfil de exemplo e bindings Python síncronos. Lance isso antes de investir em locators ricos e em perfis avançados.

---

## 13. Riscos e mitigação

| Risco | Mitigação |
|---|---|
| Mudanças no CDP quebram o cliente | Codegen reexecutável, testes em múltiplas versões |
| Detecção via vazamentos do CDP | Mundos isolados, `--remote-debugging-pipe`, evitar `Runtime.enable` no mundo principal, testar com o detector da Rebrowser |
| Perfis ficam desatualizados | Gerar o user-agent da versão real instalada, rotina de atualização |
| Auto-wait instável | Suíte de páginas de teste hostis, logs de protocolo |
| Processos órfãos | `kill_on_drop`, `PDEATHSIG`, testes de soak |
| Peso do projeto (escopo cresce) | Manter MVP enxuto, features opcionais atrás de flags |
| Detecção por camadas (TLS, CDP, JS, render, comportamento) | Mapa da seção 7.9, relatórios comparativos em CI (7.18) |
| Perfis raros ou incoerentes | Pool ponderado por participação de mercado, validação de consistência na carga |
| Uso indevido | Aviso claro, sem recursos voltados a burlar CAPTCHA ou controles de segurança |

---

## 14. Referências

- Chrome DevTools Protocol: `chromedevtools.github.io/devtools-protocol`
- Chrome for Testing: `googlechromelabs.github.io/chrome-for-testing`
- `chromiumoxide`, `headless_chrome`, `rquest` (Rust)
- `nodriver`, `zendriver` (Python)
- PyO3 e maturin: `pyo3.rs`, `maturin.rs`
- Camoufox (abordagem de Firefox patcheado, para comparação)
- `patchright`, `rebrowser-patches` (documentação de vazamentos do CDP)
- `rquest` e `curl-impersonate` (impersonação de TLS e HTTP/2)
- `curl-impersonate` (impersonação de TLS sem navegador)
- Especificação WebDriver BiDi (para suporte futuro ao Firefox)
