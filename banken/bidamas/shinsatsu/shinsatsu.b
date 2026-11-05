@moduledoc """
shinsatsu (診察) — the credential doctor: a provider, the failure classes its credential helper can show, and for each class a diagnosis, the remediation steps and a verify step. A new provider or a new failure class is a declaration.

A host hands `diagnose` the facts of one failed connect: the credential helper's program and arguments, the error text, the cluster, the apiserver and the registry target. It returns one plain record a terminal can draw and a runner can execute. Nothing here runs a command.

    diagnose({program: "kubelogin", args: [], message: "AADSTS50173: grant revoked"})[:class] #=> "grant_revoked"
    incomplete #=> []
"""

use("moji", %i[includes lower])
use("retsu", %i[find_first index_of is_empty size])

@doc """
failure: one failure class: the error markers that identify it, a short title, the diagnosis and the remediation.

    failure(:x, ["boom"], "Boom", "It broke.", fn(_f) [] end)[:title] #=> "Boom"
"""
def failure(name, markers, title, diagnosis, steps)
  {name:, markers:, title:, diagnosis:, steps:}
end

@doc """
provider: one credential provider: the helper programs it owns, the facts it reads from their arguments, and its failure classes in match order.

    provider(:p, "P", ["p"], fn(_f) [] end, [])[:label] #=> "P"
"""
def provider(name, label, programs, facts, failures)
  {name:, label:, programs:, facts:, failures:}
end

@doc """
flag: the value after a flag in an argument list, or nil.

    flag(["--a", "1", "--b", "2"], "--b") #=> "2"
    flag(["--a"], "--a") #=> nil
"""
def flag(args, name)
  i = index_of(args, name)
  if i < 0 or i + 1 >= size(args)
    nil
  else
    args[i + 1]
  end
end

def known(pairs)
  map((p) -> {label: p[0], value: p[1]}, filter((p) -> p[1] != nil, pairs))
end

def azure_facts(f)
  args = f[:args]
  known(
    [
      ["tenant", flag(args, "--tenant-id")],
      ["server id", flag(args, "--server-id")],
      ["login mode", flag(args, "--login")],
      ["client id", flag(args, "--client-id")]
    ]
  )
end

def azure_login(f)
  args = f[:args]
  tenant = flag(args, "--tenant-id")
  scope = flag(args, "--server-id")
  login = append(%w[az login], with_flag("--tenant", tenant))
  login = append(login, with_flag("--scope", scoped(scope)))
  [%w[az logout], login]
end

def with_flag(name, value)
  if value.nil?
    []
  else
    [name, value]
  end
end

def scoped(server_id)
  if server_id.nil?
    nil
  else
    "#{server_id}/.default"
  end
end

def or_else(value, fallback)
  if value.nil?
    fallback
  else
    value
  end
end

def aws_facts(f)
  args = f[:args]
  known(
    [
      ["profile", flag(args, "--profile")],
      ["region", flag(args, "--region")],
      ["cluster", flag(args, "--cluster-name")],
      ["role", flag(args, "--role-arn")]
    ]
  )
end

def aws_login(f)
  profile = flag(f[:args], "--profile")
  if profile != nil
    [["aws", "sso", "login", "--profile", profile]]
  else
    [%w[aws sso login]]
  end
end

def gcp_facts(_f)
  []
end

def gcp_login(_f)
  [%w[gcloud auth login]]
end

def nerai_facts(f)
  known([["target", f[:target]]])
end

def no_steps(_f)
  []
end

@doc """
providers: every provider the doctor knows, each with its failure classes in match order.

    size(providers) #=> 4
"""
def providers()
  [
    provider(
      :azure,
      "Azure",
      %w[kubelogin az],
      azure_facts,
      [
        failure(
          :grant_revoked,
          %w[AADSTS50173 AADSTS50133 AADSTS50132 AADSTS50089],
          "Azure login expired",
          "Entra ID revoked the sign-in grant, for example after a password reset. Sign in again.",
          azure_login
        ),
        failure(
          :token_expired,
          [
            "AADSTS700082",
            "AADSTS70043",
            "AADSTS70008",
            "refresh token has expired"
          ],
          "Azure login expired",
          "The refresh token is past its lifetime. Sign in again.",
          azure_login
        ),
        failure(
          :wrong_tenant,
          %w[AADSTS50020 AADSTS90072 AADSTS700016 AADSTS90002],
          "Azure account is in the wrong tenant",
          "The signed-in account is not in the tenant that owns the cluster. Sign in to that tenant.",
          azure_login
        ),
        failure(
          :no_subscription,
          ["No subscriptions found", "no subscription"],
          "Azure account sees no subscription",
          "The account signed in, but it sees no subscription. Sign in with an account that has access.",
          azure_login
        ),
        failure(
          :not_logged_in,
          ["Please run 'az login'", "az login", "AzureCLICredential"],
          "Azure CLI is not signed in",
          "kubelogin asks the Azure CLI for a token, and the Azure CLI has no account. Sign in.",
          azure_login
        )
      ]
    ),
    provider(
      :aws,
      "AWS",
      %w[aws aws-iam-authenticator],
      aws_facts,
      [
        failure(
          :sso_expired,
          [
            "SSO session associated with this profile has expired",
            "Error when retrieving token from sso",
            "Token has expired and refresh failed",
            "token is expired"
          ],
          "AWS SSO session expired",
          "The AWS SSO session is past its lifetime. Sign in again.",
          aws_login
        ),
        failure(
          :no_credentials,
          [
            "Unable to locate credentials",
            "could not be found",
            "ExpiredToken"
          ],
          "AWS has no usable credentials",
          "The AWS CLI found no credentials for this profile. Sign in.",
          aws_login
        )
      ]
    ),
    provider(
      :gcp,
      "Google Cloud",
      %w[gke-gcloud-auth-plugin gcloud],
      gcp_facts,
      [
        failure(
          :reauth,
          [
            "Reauthentication failed",
            "reauthentication",
            "invalid_grant",
            "gcloud auth login"
          ],
          "Google Cloud login expired",
          "The gcloud credential needs a new sign-in. Sign in again.",
          gcp_login
        ),
        failure(
          :no_adc,
          [
            "could not find default credentials",
            "Application Default Credentials"
          ],
          "Google Cloud has no default credentials",
          "No application default credential exists. Sign in.",
          fn(_f)
            [%w[gcloud auth login], %w[gcloud auth application-default login]]
          end
        )
      ]
    ),
    provider(
      :nerai,
      "nerai",
      ["nerai-refuse"],
      nerai_facts,
      [
        failure(
          :refused,
          %w[nerai: refused],
          "Refused by the target registry",
          "The ambient kubeconfig refuses this name on purpose. Open the registry target, which carries its own kubeconfig.",
          no_steps
        )
      ]
    )
  ]
end

def owns(p, program)
  index_of(p[:programs], program) >= 0
end

def matches(c, text)
  low = lower(text)
  find_first((m) -> includes(low, lower(m)), c[:markers]) != nil
end

def verify_of(f)
  if f[:program].nil? or f[:program] == "" or f[:program] == "nerai-refuse"
    []
  else
    append([f[:program]], f[:args])
  end
end

def record(p, c, f)
  {
    provider: to_s(p[:name]),
    label: p[:label],
    class: to_s(c[:name]),
    title: c[:title],
    diagnosis: c[:diagnosis],
    facts: append(
      known([["cluster", f[:cluster]], ["apiserver", f[:server]]]),
      p[:facts](f)
    ),
    steps: map((argv) -> {argv:}, c[:steps](f)),
    verify: verify_of(f)
  }
end

@doc """
diagnose: the record for one failed connect, or nil when no class matches. `facts` takes {program:, args:, message:, cluster:, server:, target:}.

    diagnose({program: "aws", args: ["--profile", "p"], message: "Error when retrieving token from sso"})[:steps] #=> [{argv: ["aws", "sso", "login", "--profile", "p"]}]
"""
def diagnose(facts)
  f = {
    program: or_else(facts[:program], ""),
    args: or_else(facts[:args], []),
    message: or_else(facts[:message], ""),
    cluster: facts[:cluster],
    server: facts[:server],
    target: facts[:target]
  }
  owned = filter((p) -> owns(p, f[:program]), providers)
  found = first_match(owned, f)
  if found.nil?
    first_match(providers, f)
  else
    found
  end
end

def first_match(pool, f)
  reduce(
    fn(found, p)
      if found != nil
        found
      else
        c = find_first((c) -> matches(c, f[:message]), p[:failures])
        if c.nil?
          nil
        else
          record(p, c, f)
        end
      end
    end,
    nil,
    pool
  )
end

@doc """
classes: every failure class, as {provider:, class:} records.

    size(classes) > 0 #=> true
"""
def classes()
  reduce(
    fn(acc, p)
      append(
        acc,
        map(
          (c) -> {provider: to_s(p[:name]), class: to_s(c[:name])},
          p[:failures]
        )
      )
    end,
    [],
    providers
  )
end

@doc """
incomplete: every failure class that lacks a marker, a title, a diagnosis or a remediation. Remediation is a step, or the nerai answer to open the registry target.

    incomplete #=> []
"""
def incomplete()
  probe = {
    program: "x",
    args: [],
    message: "",
    cluster: nil,
    server: nil,
    target: "t"
  }
  reduce(
    fn(acc, p)
      append(
        acc,
        map(
          (c) -> "#{p[:name]}/#{c[:name]}",
          filter(
            fn(c)
              is_empty(c[:markers]) or
                c[:title] == "" or
                c[:diagnosis] == "" or
                is_empty(c[:steps](probe)) and p[:name] != :nerai
            end,
            p[:failures]
          )
        )
      )
    end,
    [],
    providers
  )
end

@doc """
diagnose_json: `diagnose` over JSON text, for a host that speaks JSON.

    diagnose_json("{}") #=> "null"
"""
def diagnose_json(text)
  json_stringify(diagnose(keyed(json_parse(text))))
end

def keyed(m)
  {
    program: json_get_or(m, :program, ""),
    args: json_get_or(m, :args, []),
    message: json_get_or(m, :message, ""),
    cluster: json_get_or(m, :cluster, nil),
    server: json_get_or(m, :server, nil),
    target: json_get_or(m, :target, nil)
  }
end

def classes_json()
  json_stringify(classes)
end

def incomplete_json()
  json_stringify(incomplete)
end
