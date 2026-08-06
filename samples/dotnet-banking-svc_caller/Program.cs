using NexGen.BankService;
using Temporalio.Client;
using TemporalioSamples.BankingSvcCaller;

// Accounts are hard-coded and the contract has no delete, so they outlive a run.
// Every transfer below is matched by one back the other way, leaving the balances
// where they started — the walkthrough reads the same on the first run and the
// hundredth.
const string Chris = "chris-checking";
const string Merchant = "acme-merchant";
const string Missing = "no-such-account";

Console.WriteLine("Hello, World");

await RunWalkthroughAsync();

async Task RunWalkthroughAsync()
{
    var client = await ConnectAsync();

    // IBankService and every input/output below are emitted by nex-gen from
    // samples/schemas/banking-service.yaml. Nothing here is hand-written.
    var bank = client.CreateNexusClient<IBankService>(NexusEndpoints.BankService);

    Section(
        "Open the accounts",
        "CreateAccount is idempotent here: on a repeat run the handler returns the",
        "account's current balance rather than resetting it. `amount` is optional in",
        "the contract with a default of 500, so the merchant leaves it off.");
    await CreateAccountAsync(bank, Chris, 1000);
    await CreateAccountAsync(bank, Merchant, null);

    Section(
        "Read the starting balances",
        "GetBalance on each account.");
    await GetBalanceAsync(bank, Chris);
    await GetBalanceAsync(bank, Merchant);

    Section(
        "Move money",
        "A transfer the handler can satisfy. The contract models the outcome as",
        "data — `success` plus an optional `errorMessage` — not as an error.");
    await SendMoneyAsync(bank, Chris, Merchant, 250);
    await GetBalanceAsync(bank, Chris);
    await GetBalanceAsync(bank, Merchant);

    Section(
        "Now try to overdraw the account",
        "Send more than the account holds. The amount is well within the contract's",
        "bounds, so this is a business rule the handler enforces, not a contract",
        "violation. Comes back as success=False with a reason.");
    await SendMoneyAsync(bank, Chris, Merchant, 100000);

    Section(
        "Send to an account that does not exist",
        "Another business-rule rejection from the handler, reported the same way.");
    await SendMoneyAsync(bank, Chris, Missing, 10);

    Section(
        "Break the contract, caught before the wire",
        "`amount` is declared `exclusiveMinimum: 0`, so 0 is invalid. Calling",
        "Validate() on the generated model rejects it locally — the request is never",
        "sent, and every violation is reported at once rather than the first only.");
    ValidateLocally(new TransferMoneyInput(Chris, Merchant, 0));

    Section(
        "Break two rules at once",
        "2^53 exceeds both the contract's `maximum: 100000` and the largest integer",
        "JSON carries losslessly. Both violations arrive in a single exception —",
        "the validator never stops at the first one.");
    ValidateLocally(new TransferMoneyInput(Chris, Merchant, 9007199254740992));

    Section(
        "Skip the local check and let it reach the handler",
        "Same out-of-bounds amount, but sent without calling Validate() first. The",
        "handler does not accept it, and because the failure is retried the call",
        "ends at its deadline rather than returning a crisp error. Validating",
        "locally is both faster and far more specific — that is the point of the",
        "generated validator.");
    await SendMoneyAsync(bank, Chris, Merchant, 200000, TimeSpan.FromSeconds(5));

    Section(
        "Put the money back",
        "Restores the opening balances so this sample can be run repeatedly.");
    await SendMoneyAsync(bank, Merchant, Chris, 250);
    await GetBalanceAsync(bank, Chris);
    await GetBalanceAsync(bank, Merchant);
}

// Temporal Cloud: always TLS, always an API key. Reading NexusEndpoints.ApiKey
// throws if TEMPORAL_API_KEY is unset, which is the intent — there is no
// unauthenticated fallback.
async Task<TemporalClient> ConnectAsync() =>
    await TemporalClient.ConnectAsync(new(NexusEndpoints.Address)
    {
        Namespace = NexusEndpoints.Namespace,
        Tls = new(),
        ApiKey = NexusEndpoints.ApiKey,
    });

async Task CreateAccountAsync(NexusClient<IBankService> bank, string accountId, long? amount)
{
    // Built outside the lambda: StartNexusOperationAsync takes an expression tree,
    // which cannot contain a conditional pattern match.
    var input = new CreateAccountInput(accountId);
    if (amount is not null)
    {
        input = new CreateAccountInput(accountId) { Amount = amount };
    }

    try
    {
        var handle = await bank.StartNexusOperationAsync(
            svc => svc.CreateAccount(input),
            NewOptions($"create-{accountId}"));
        var created = await handle.GetResultAsync();
        Console.WriteLine(
            $"  CreateAccount({accountId}) -> amount={created.Amount}");
    }
    catch (Exception ex)
    {
        // A handler that rejects an existing account is free to signal it however
        // it likes, and that is not fatal here — the account exists either way.
        Console.WriteLine($"  CreateAccount({accountId}) -> skipped: {Describe(ex)}");
    }
}

async Task GetBalanceAsync(NexusClient<IBankService> bank, string accountId)
{
    var input = new GetBalanceInput(accountId);
    var handle = await bank.StartNexusOperationAsync(
        svc => svc.GetBalance(input),
        NewOptions($"balance-{accountId}"));
    var balance = await handle.GetResultAsync();
    Console.WriteLine($"  GetBalance({accountId}) -> amount={balance.Amount}");
}

async Task SendMoneyAsync(
    NexusClient<IBankService> bank,
    string from,
    string to,
    long amount,
    TimeSpan? timeout = null)
{
    var input = new TransferMoneyInput(from, to, amount);
    try
    {
        var handle = await bank.StartNexusOperationAsync(
            svc => svc.SendMoney(input),
            NewOptions("send-money", timeout));
        var transfer = await handle.GetResultAsync();
        Console.WriteLine(
            $"  SendMoney({from} -> {to}, {amount}) -> success={transfer.Success}" +
            (transfer.ErrorMessage is null ? string.Empty : $", error=\"{transfer.ErrorMessage}\""));
    }
    catch (Exception ex)
    {
        Console.WriteLine($"  SendMoney({from} -> {to}, {amount}) -> rejected: {Describe(ex)}");
    }
}

// Runs the generated validator without calling the service at all.
void ValidateLocally(TransferMoneyInput input)
{
    try
    {
        input.Validate();
        Console.WriteLine($"  Validate(amount={input.Amount}) -> passed, would be sent");
    }
    catch (ValidationException ex)
    {
        Console.WriteLine(
            $"  Validate(amount={input.Amount}) -> {ex.Violations.Count} violation(s), not sent");
        foreach (var violation in ex.Violations)
        {
            Console.WriteLine($"      {violation}");
        }
    }
}

// Operation ids must be unique per call, so each gets a fresh suffix. Account ids
// stay stable; only the operation id varies.
NexusOperationOptions NewOptions(string label, TimeSpan? timeout = null) =>
    new($"banking-{label}-{Guid.NewGuid():N}")
    {
        ScheduleToCloseTimeout = timeout ?? TimeSpan.FromSeconds(30),
    };

void Section(string title, params string[] description)
{
    Console.WriteLine();
    Console.WriteLine($"## {title}");
    foreach (var line in description)
    {
        Console.WriteLine(line);
    }
    Console.WriteLine();
}

// Nexus failures nest the useful text one or two levels down.
static string Describe(Exception ex) =>
    (ex.InnerException?.InnerException ?? ex.InnerException ?? ex).Message;
