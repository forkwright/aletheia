// TLS certificate generation and server setup
import { generateKeyPairSync, randomBytes } from "node:crypto";
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { createServer as createHttpsServer } from "node:https";
import { createLogger } from "../koina/logger.js";

const log = createLogger("tls");

export interface TlsConfig {
  enabled: boolean;
  mode: "auto" | "provided";
  autoSubjectAltNames?: string[];
  certFile?: string;
  keyFile?: string;
}

export interface TlsCerts {
  cert: string;
  key: string;
}

function generateSelfSignedCert(sans: string[]): TlsCerts {
  const { privateKey } = generateKeyPairSync("ec", {
    namedCurve: "prime256v1",
  });

  // Use node:crypto X509Certificate approach — but for self-signed generation
  // we need openssl-like capabilities. Use a simpler ASN.1 approach.
  // In practice, spawn openssl for cert generation since node:crypto
  // doesn't have a full X509 builder.
  const { execSync } = require("node:child_process") as typeof import("node:child_process");

  const tmpDir = join(require("node:os").tmpdir(), `aletheia-tls-${randomBytes(4).toString("hex")}`);
  mkdirSync(tmpDir, { recursive: true });

  const keyPath = join(tmpDir, "server.key");
  const certPath = join(tmpDir, "server.crt");
  const confPath = join(tmpDir, "openssl.cnf");

  // Write the private key
  const keyPem = privateKey.export({ type: "sec1", format: "pem" }) as string;
  writeFileSync(keyPath, keyPem, { mode: 0o600 });

  // Build SANs config
  const sanEntries = sans.map((s, i) => {
    if (/^\d+\.\d+\.\d+\.\d+$/.test(s)) {
      return `IP.${i + 1} = ${s}`;
    }
    return `DNS.${i + 1} = ${s}`;
  });

  const conf = `
[req]
default_bits = 2048
prompt = no
default_md = sha256
distinguished_name = dn
x509_extensions = v3_req

[dn]
CN = Aletheia

[v3_req]
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = @alt_names

[alt_names]
${sanEntries.join("\n")}
`;

  writeFileSync(confPath, conf);

  try {
    execSync(
      `openssl req -new -x509 -key "${keyPath}" -out "${certPath}" -days 365 -config "${confPath}"`,
      { stdio: "pipe" },
    );
  } catch (err) {
    log.error("Failed to generate self-signed cert — is openssl installed?");
    throw err;
  }

  const cert = readFileSync(certPath, "utf-8");
  const key = keyPem;

  // Cleanup temp files
  try {
    const { rmSync } = require("node:fs") as typeof import("node:fs");
    rmSync(tmpDir, { recursive: true });
  } catch {
    // not critical
  }

  return { cert, key };
}

export function loadOrGenerateCerts(
  tlsConfig: TlsConfig,
  credentialsDir: string,
): TlsCerts | null {
  if (!tlsConfig.enabled) return null;

  if (tlsConfig.mode === "provided") {
    if (!tlsConfig.certFile || !tlsConfig.keyFile) {
      throw new Error(
        "TLS mode 'provided' requires certFile and keyFile",
      );
    }
    return {
      cert: readFileSync(tlsConfig.certFile, "utf-8"),
      key: readFileSync(tlsConfig.keyFile, "utf-8"),
    };
  }

  // Auto mode — generate self-signed
  const tlsDir = join(credentialsDir, "tls");
  const certPath = join(tlsDir, "server.crt");
  const keyPath = join(tlsDir, "server.key");

  if (existsSync(certPath) && existsSync(keyPath)) {
    log.info("Loading existing self-signed TLS certificate");
    return {
      cert: readFileSync(certPath, "utf-8"),
      key: readFileSync(keyPath, "utf-8"),
    };
  }

  log.info("Generating self-signed TLS certificate");
  const sans = tlsConfig.autoSubjectAltNames ?? ["localhost"];
  const certs = generateSelfSignedCert(sans);

  mkdirSync(tlsDir, { recursive: true });
  writeFileSync(certPath, certs.cert, { mode: 0o644 });
  writeFileSync(keyPath, certs.key, { mode: 0o600 });
  log.info(`TLS certificate written to ${tlsDir}`);

  return certs;
}

export function createTlsServer(certs: TlsCerts) {
  return createHttpsServer({
    cert: certs.cert,
    key: certs.key,
  });
}
