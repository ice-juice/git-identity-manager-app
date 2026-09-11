import { useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { BookOpen, ExternalLink, X } from "lucide-react";
import { api } from "../lib/ipc";

export type S3GuideProvider = "r2" | "s3" | "minio" | "other";

type Step = {
  title: string;
  body: string;
  url?: { href: string; label: string };
};

const PROVIDERS: { id: S3GuideProvider; label: string; hint: string }[] = [
  { id: "r2", label: "Cloudflare R2", hint: "推荐新手" },
  { id: "s3", label: "AWS S3", hint: "亚马逊云" },
  { id: "minio", label: "MinIO", hint: "自己电脑/服务器" },
  { id: "other", label: "其他兼容盘", hint: "阿里云 / 腾讯云等" },
];

const INTRO =
  "本软件不会帮你开云账号。你要先在云厂商那里准备一个「储物柜」（存储桶）和两把「钥匙」（Access Key / Secret Key），再填回下面的表格。数据会先在本机加密再上传，云厂商看不到内容。";

const STEPS: Record<S3GuideProvider, { blurb: string; steps: Step[]; fill: string[] }> = {
  r2: {
    blurb:
      "Cloudflare R2 和网盘很像，个人备份用量通常走免费额度。开通时可能要绑一张银行卡，但用得少一般不会扣钱。下面每一步做完，再做下一步。",
    steps: [
      {
        title: "注册并登录 Cloudflare",
        body: "打开 Cloudflare 官网，用邮箱注册一个免费账号并登录。如果已经有账号，直接登录即可。",
        url: { href: "https://dash.cloudflare.com/sign-up", label: "去注册 / 登录" },
      },
      {
        title: "打开 R2 对象存储",
        body: "登录后，在控制台左侧找到「存储和数据库」或直接搜 R2。点进 R2 概览。第一次使用可能提示「开通 R2」，按页面提示开通。",
        url: { href: "https://dash.cloudflare.com/?to=/:account/r2/overview", label: "打开 R2 概览" },
      },
      {
        title: "创建一个存储桶",
        body: "点「创建存储桶 / Create bucket」。名字只能用小写字母、数字和连字符，例如 git-keymaster-backup。位置选 Automatic（自动）即可。不要把桶设成公开。创建后记住这个名字，它就是本软件里的「存储桶名称」。",
      },
      {
        title: "创建 API 令牌（两把钥匙）",
        body: "回到 R2 概览，在账户详情附近点「管理 API 令牌 / Manage」。再点「创建 API 令牌」。权限选 Object Read & Write（对象读和写）。范围尽量只勾选刚才那个桶。点创建。",
      },
      {
        title: "立刻抄下三样东西",
        body: "创建成功后，页面会显示 Access Key ID、Secret Access Key，以及 Endpoint（形如 https://一长串数字字母.r2.cloudflarestorage.com）。Secret 只出现这一次，关掉就看不到了，请先复制到记事本。账户 ID 也可在 R2 概览页找到，用来拼出端点。",
      },
      {
        title: "填回本软件",
        body: "存储端点填 Endpoint 整段网址（不要漏 https://）。存储桶填桶名。区域填 auto。路径前缀建议保持 gam-sync/，多台电脑必须完全一样。两把钥匙分别填 Access Key ID 和 Secret Access Key。填完先「保存配置」，再点「测试连通性」。",
      },
    ],
    fill: [
      "存储端点 = https://你的账户ID.r2.cloudflarestorage.com",
      "存储桶名称 = 刚才创建的桶名",
      "区域 = auto",
      "路径前缀 = gam-sync/",
    ],
  },
  s3: {
    blurb:
      "AWS S3 是亚马逊的对象存储。注册通常要国际信用卡，按用量计费。只要用来备份本软件的加密数据包，容量很小，费用一般很低。不要用「根账号」的密钥。",
    steps: [
      {
        title: "注册并登录 AWS",
        body: "打开 AWS 控制台注册账号。按提示完成邮箱、手机和付款方式验证。登录后先看一眼右上角的区域，例如新加坡 ap-southeast-1、东京 ap-northeast-1、美国东部 us-east-1。",
        url: { href: "https://console.aws.amazon.com/", label: "打开 AWS 控制台" },
      },
      {
        title: "创建一个 S3 存储桶",
        body: "在控制台搜索 S3 并打开。点「创建存储桶」。桶名全球唯一，用小写和连字符，例如 git-keymaster-backup-你的名字缩写。区域选离你近的一个，并记住它。屏蔽所有公共访问保持开启。其他选项用默认即可。",
        url: { href: "https://s3.console.aws.amazon.com/s3/home", label: "打开 S3" },
      },
      {
        title: "建一个只能管这个桶的 IAM 用户",
        body: "不要用登录密码当密钥。打开 IAM → 用户 → 创建用户。用户名例如 git-keymaster-sync。创建后进入该用户 →「安全凭证」→「创建访问密钥」，用途选「第三方应用程序」或「本地代码」。",
        url: { href: "https://console.aws.amazon.com/iam/home#/users", label: "打开 IAM 用户" },
      },
      {
        title: "给这个用户开 S3 权限",
        body: "回到该用户的「权限」标签，附加策略。新手可先附加 AmazonS3FullAccess 做通；更稳妥的做法是只允许这一个桶的 s3:ListBucket / GetObject / PutObject / DeleteObject。没有权限时，本软件测试连通会失败。",
      },
      {
        title: "抄下 Access Key 和 Secret",
        body: "创建访问密钥后立刻复制 Access Key ID（一般以 AKIA 开头）和 Secret Access Key。Secret 只显示一次。",
      },
      {
        title: "填回本软件",
        body: "存储端点填 https://s3.区域代码.amazonaws.com，例如新加坡是 https://s3.ap-southeast-1.amazonaws.com。区域填同一段区域代码。存储桶填桶名。路径前缀建议 gam-sync/。填完保存并测试连通性。",
      },
    ],
    fill: [
      "存储端点 = https://s3.你的区域.amazonaws.com",
      "存储桶名称 = 刚才创建的桶名",
      "区域 = 创建桶时选的区域，例如 ap-southeast-1",
      "路径前缀 = gam-sync/",
    ],
  },
  minio: {
    blurb:
      "MinIO 是装在你自己电脑或局域网服务器上的 S3 兼容存储。适合完全不想把备份放到公有云的人。需要你先自己把 MinIO 跑起来。",
    steps: [
      {
        title: "安装并启动 MinIO",
        body: "到 MinIO 官网下载对应系统的程序，按官方文档启动。默认控制台一般是 http://127.0.0.1:9001，S3 接口是 http://127.0.0.1:9000。第一次启动时会打印或让你设置登录用户和密码。",
        url: { href: "https://min.io/download", label: "打开 MinIO 下载页" },
      },
      {
        title: "登录控制台并建桶",
        body: "用浏览器打开控制台，登录后点「Create Bucket」。桶名同样用小写和连字符。不要设成公开下载。",
      },
      {
        title: "准备访问密钥",
        body: "在控制台的 Access Keys 里创建一组密钥，或使用启动时设置的 root 用户名当作 Access Key ID、密码当作 Secret Access Key（仅限自己电脑上的测试环境）。",
      },
      {
        title: "填回本软件",
        body: "本机 MinIO 的存储端点一般是 http://127.0.0.1:9000。如果 MinIO 在另一台机器，改成那台机器的 IP 或域名，并加上端口。区域填 us-east-1（MinIO 通常不校验，但本软件需要填一项）。存储桶填桶名，前缀建议 gam-sync/。",
      },
    ],
    fill: [
      "存储端点 = http://127.0.0.1:9000（或你的服务器地址）",
      "存储桶名称 = 控制台里创建的桶名",
      "区域 = us-east-1",
      "路径前缀 = gam-sync/",
    ],
  },
  other: {
    blurb:
      "阿里云 OSS、腾讯云 COS、又拍云、Cloudflare 以外的很多对象存储，只要提供「S3 兼容接口」就能用。页面名称可能叫 AccessKey、SecretKey、Endpoint、Bucket。",
    steps: [
      {
        title: "确认开通了 S3 兼容",
        body: "在云厂商控制台里找到对象存储，确认文档写着兼容 Amazon S3 API。有的要单独打开「S3 兼容」开关。",
      },
      {
        title: "创建存储桶，关掉公开访问",
        body: "创建一个私有桶。记下桶名和所在地域（例如 oss-cn-hangzhou、ap-guangzhou）。",
      },
      {
        title: "创建子账号密钥，不要用主账号",
        body: "在访问控制 / CAM / RAM 里建一个只能访问这个桶的子用户，再创建 AccessKey。主账号密钥泄漏风险更大。",
      },
      {
        title: "对照厂商文档抄 Endpoint",
        body: "Endpoint 必须是 S3 兼容地址，通常带 https://，有的还带地域，例如 https://oss-cn-hangzhou.aliyuncs.com。不要填控制台网页地址，也不要填自定义域名（除非厂商明确说它走 S3 API）。",
      },
      {
        title: "填回本软件",
        body: "存储端点 = 厂商给的 S3 Endpoint。存储桶 = 桶名。区域 = 文档要求的 region（有的填 auto 也能通，不通就改成地域代码）。路径前缀建议 gam-sync/。填完保存并测试。",
      },
    ],
    fill: [
      "存储端点 = 厂商文档里的 S3 Endpoint",
      "存储桶名称 = 桶名",
      "区域 = 文档中的 region，或先试 auto",
      "路径前缀 = gam-sync/",
    ],
  },
};

export function S3SetupGuide({
  open,
  initial = "r2",
  onClose,
  onApplyPreset,
}: {
  open: boolean;
  initial?: S3GuideProvider;
  onClose: () => void;
  onApplyPreset?: (type: Exclude<S3GuideProvider, "other">) => void;
}) {
  const [tab, setTab] = useState<S3GuideProvider>(initial);

  useEffect(() => {
    if (open) setTab(initial);
  }, [open, initial]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  const pack = STEPS[tab];
  const canFill = tab !== "other" && onApplyPreset;

  return createPortal(
    <div className="wizard-overlay" role="dialog" aria-modal="true" aria-labelledby="s3-guide-title" onClick={onClose}>
      <div className="card dialog-card s3-guide" onClick={(e) => e.stopPropagation()}>
        <div className="card-head">
          <div className="card-title" id="s3-guide-title">
            云存储小白引导
          </div>
          <button type="button" className="btn ghost sm" onClick={onClose} aria-label="关闭">
            <X size={14} />
          </button>
        </div>
        <div className="card-body s3-guide-body">
          <p className="s3-guide-intro">{INTRO}</p>
          <div className="s3-guide-tabs" role="tablist">
            {PROVIDERS.map((p) => (
              <button
                key={p.id}
                type="button"
                role="tab"
                aria-selected={tab === p.id}
                className={"s3-guide-tab" + (tab === p.id ? " on" : "")}
                onClick={() => setTab(p.id)}
              >
                <span>{p.label}</span>
                <em>{p.hint}</em>
              </button>
            ))}
          </div>
          <p className="muted s3-guide-blurb">{pack.blurb}</p>
          <ol className="s3-guide-steps">
            {pack.steps.map((s, i) => (
              <li key={s.title}>
                <span className="s3-guide-num">{i + 1}</span>
                <div>
                  <div className="s3-guide-step-title">{s.title}</div>
                  <p>{s.body}</p>
                  {s.url && (
                    <button
                      type="button"
                      className="btn ghost sm s3-guide-link"
                      onClick={() => api.openUrl(s.url!.href)}
                    >
                      <ExternalLink size={12} />
                      {s.url.label}
                    </button>
                  )}
                </div>
              </li>
            ))}
          </ol>
          <div className="s3-guide-map">
            <div className="s3-guide-step-title">对照填写</div>
            <ul>
              {pack.fill.map((line) => (
                <li key={line}>{line}</li>
              ))}
            </ul>
            <p className="muted sm" style={{ marginTop: 6 }}>
              两把钥匙请自己保管，不要发到聊天或邮件。换电脑恢复时，这 6 项必须和旧设备完全一致（含路径前缀）。
            </p>
          </div>
        </div>
        <div className="card-foot">
          <span className="muted sm">做完后回到表格，保存并测试连通性。</span>
          <div className="row" style={{ gap: 6 }}>
            {canFill && (
              <button
                type="button"
                className="btn sm"
                onClick={() => {
                  onApplyPreset(tab);
                  onClose();
                }}
              >
                填入该平台模板
              </button>
            )}
            <button type="button" className="btn primary sm" onClick={onClose}>
              我去填表了
            </button>
          </div>
        </div>
      </div>
    </div>,
    document.body,
  );
}

export function S3GuideButton({
  onClick,
}: {
  onClick: () => void;
}) {
  return (
    <button type="button" className="btn ghost sm" onClick={onClick} title="逐步教你如何建桶、拿密钥">
      <BookOpen size={13} />
      小白引导
    </button>
  );
}
