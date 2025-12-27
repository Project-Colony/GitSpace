using Microsoft.Extensions.Logging;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Tmds.DBus;
#if WINDOWS
using System.Windows.Forms;
#else
using NativeFileDialogSharp;
#endif

using var loggerFactory = LoggerFactory.Create(builder =>
{
    builder
        .SetMinimumLevel(LogLevel.Information)
        .AddSimpleConsole(options =>
        {
            options.SingleLine = true;
            options.TimestampFormat = "HH:mm:ss ";
        });
});

var logger = loggerFactory.CreateLogger("GitSpace.Helper");

var input = await Console.In.ReadToEndAsync();
if (string.IsNullOrWhiteSpace(input))
{
    logger.LogWarning("No JSON request provided on stdin.");
    return;
}

Request? request;
try
{
    request = JsonSerializer.Deserialize<Request>(input);
}
catch (JsonException ex)
{
    logger.LogError(ex, "Invalid JSON payload.");
    return;
}

if (request is null)
{
    logger.LogError("Request payload was empty after deserialization.");
    return;
}

var response = request.Command.ToLowerInvariant() switch
{
    "ping" => Response.Ok(request.Id, new
    {
        version = Assembly.GetExecutingAssembly().GetName().Version?.ToString() ?? "0.1.0"
    }),
    "dialog.open" => HandleDialogOpen(request),
    "credential.request" => HandleCredentialRequest(request),
    "library.call" => HandleLibraryCall(request),
    _ => Response.Error(
        request.Id,
        "InvalidRequest",
        "Unknown command",
        new { command = request.Command })
};

Console.WriteLine(JsonSerializer.Serialize(response));

static Response HandleDialogOpen(Request request)
{
    DialogOpenRequest? payload;
    try
    {
        payload = JsonSerializer.Deserialize<DialogOpenRequest>(
            request.Payload.GetRawText(),
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
    }
    catch (JsonException ex)
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            $"Malformed dialog payload: {ex.Message}",
            null);
    }

    if (payload is null || string.IsNullOrWhiteSpace(payload.Kind))
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            "Missing payload.kind",
            new { field = "kind" });
    }

    var title = payload.Title ?? string.Empty;
    var filters = payload.Filters ?? Array.Empty<DialogFilter>();
    var options = payload.Options ?? new DialogOptions(false, false);

    try
    {
#if WINDOWS
        var result = payload.Kind switch
        {
            "open_file" => OpenFileDialogWindows(title, filters, options),
            "open_folder" => OpenFolderDialogWindows(title, options),
            "save_file" => SaveFileDialogWindows(title, filters, options),
            _ => throw new InvalidOperationException($"Unsupported dialog kind '{payload.Kind}'.")
        };
#else
        var result = payload.Kind switch
        {
            "open_file" => OpenFileDialogNative(filters, options),
            "open_folder" => OpenFolderDialogNative(),
            "save_file" => SaveFileDialogNative(filters),
            _ => throw new InvalidOperationException($"Unsupported dialog kind '{payload.Kind}'.")
        };
#endif

        return Response.Ok(request.Id, new
        {
            selected_paths = result.Paths,
            cancelled = result.Cancelled
        });
    }
    catch (InvalidOperationException ex)
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            ex.Message,
            new { kind = payload.Kind });
    }
    catch (Exception ex)
    {
        return Response.Error(
            request.Id,
            "Internal",
            "Unhandled exception",
            new { error = ex.Message });
    }
}

static Response HandleCredentialRequest(Request request)
{
    CredentialRequest? payload;
    try
    {
        payload = JsonSerializer.Deserialize<CredentialRequest>(
            request.Payload.GetRawText(),
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
    }
    catch (JsonException ex)
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            $"Malformed credential payload: {ex.Message}",
            null);
    }

    if (payload is null || string.IsNullOrWhiteSpace(payload.Service))
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            "Missing payload.service",
            new { field = "service" });
    }

    if (string.IsNullOrWhiteSpace(payload.Action))
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            "Missing payload.action",
            new { field = "action" });
    }

    var action = payload.Action.ToLowerInvariant();
    if (action is not ("get" or "store" or "erase"))
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            "Unsupported payload.action",
            new { action = payload.Action });
    }

    try
    {
        var provider = CredentialProviderFactory.Create();
        var result = action switch
        {
            "get" => provider.Get(payload.Service, payload.Account),
            "store" => provider.Store(payload.Service, payload.Account, payload.Secret),
            "erase" => provider.Erase(payload.Service, payload.Account),
            _ => new CredentialPayload(null, null, CredentialStatus.Denied)
        };

        return Response.Ok(request.Id, new
        {
            username = result.Username,
            secret = result.Secret,
            status = result.Status
        });
    }
    catch (Exception ex)
    {
        return Response.Error(
            request.Id,
            "Internal",
            "Unhandled exception",
            new { error = ex.Message });
    }
}

static Response HandleLibraryCall(Request request)
{
    LibraryCallRequest? payload;
    try
    {
        payload = JsonSerializer.Deserialize<LibraryCallRequest>(
            request.Payload.GetRawText(),
            new JsonSerializerOptions { PropertyNameCaseInsensitive = true });
    }
    catch (JsonException ex)
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            $"Malformed library payload: {ex.Message}",
            null);
    }

    if (payload is null || string.IsNullOrWhiteSpace(payload.Name))
    {
        return Response.Error(
            request.Id,
            "InvalidRequest",
            "Missing payload.name",
            new { field = "name" });
    }

    var name = payload.Name.ToLowerInvariant();
    return name switch
    {
        "system.info" => Response.Ok(request.Id, new
        {
            os = RuntimeInformation.OSDescription,
            version = Environment.OSVersion.VersionString
        }),
        _ => Response.Error(
            request.Id,
            "InvalidRequest",
            "Unknown library name",
            new { name = payload.Name })
    };
}

static string BuildWindowsFilter(DialogFilter[] filters)
{
    if (filters.Length == 0)
    {
        return "All Files|*.*";
    }

    return string.Join("|", filters.Select(filter =>
    {
        var extensions = filter.Extensions.Length == 0
            ? "*.*"
            : string.Join(";", filter.Extensions.Select(ext => $"*.{ext.TrimStart('.')}"));
        return $"{filter.Label}|{extensions}";
    }));
}

static string BuildNativeFilter(DialogFilter[] filters)
{
    var extensions = filters
        .SelectMany(filter => filter.Extensions)
        .Select(ext => ext.TrimStart('.'))
        .Where(ext => !string.IsNullOrWhiteSpace(ext))
        .Distinct(StringComparer.OrdinalIgnoreCase)
        .ToArray();

    return extensions.Length == 0 ? string.Empty : string.Join(",", extensions);
}

static void TrySetProperty(object target, string propertyName, object value)
{
    var property = target.GetType().GetProperty(propertyName);
    if (property is not null && property.CanWrite)
    {
        property.SetValue(target, value);
    }
}

#if WINDOWS
static DialogOpenResult OpenFileDialogWindows(string title, DialogFilter[] filters, DialogOptions options)
{
    using var dialog = new OpenFileDialog
    {
        Title = title,
        Filter = BuildWindowsFilter(filters),
        Multiselect = options.MultiSelect
    };
    TrySetProperty(dialog, "ShowHiddenItems", options.ShowHidden);
    TrySetProperty(dialog, "ShowHidden", options.ShowHidden);

    return dialog.ShowDialog() == System.Windows.Forms.DialogResult.OK
        ? new DialogOpenResult(dialog.FileNames, false)
        : DialogOpenResult.CancelledResult;
}

static DialogOpenResult SaveFileDialogWindows(string title, DialogFilter[] filters, DialogOptions options)
{
    using var dialog = new SaveFileDialog
    {
        Title = title,
        Filter = BuildWindowsFilter(filters)
    };
    TrySetProperty(dialog, "ShowHiddenItems", options.ShowHidden);
    TrySetProperty(dialog, "ShowHidden", options.ShowHidden);

    return dialog.ShowDialog() == System.Windows.Forms.DialogResult.OK && !string.IsNullOrWhiteSpace(dialog.FileName)
        ? new DialogOpenResult(new[] { dialog.FileName }, false)
        : DialogOpenResult.CancelledResult;
}

static DialogOpenResult OpenFolderDialogWindows(string title, DialogOptions options)
{
    using var dialog = new FolderBrowserDialog
    {
        Description = title,
        ShowNewFolderButton = true
    };
    TrySetProperty(dialog, "ShowHiddenFiles", options.ShowHidden);

    return dialog.ShowDialog() == System.Windows.Forms.DialogResult.OK && !string.IsNullOrWhiteSpace(dialog.SelectedPath)
        ? new DialogOpenResult(new[] { dialog.SelectedPath }, false)
        : DialogOpenResult.CancelledResult;
}
#else
static DialogOpenResult OpenFileDialogNative(DialogFilter[] filters, DialogOptions options)
{
    var filterList = BuildNativeFilter(filters);
    if (options.MultiSelect)
    {
        var result = Nfd.OpenDialogMultiple(out var paths, filterList, null);
        return result == NfdResult.Ok && paths is { Length: > 0 }
            ? new DialogOpenResult(paths, false)
            : DialogOpenResult.CancelledResult;
    }

    var singleResult = Nfd.OpenDialog(out var path, filterList, null);
    return singleResult == NfdResult.Ok && !string.IsNullOrWhiteSpace(path)
        ? new DialogOpenResult(new[] { path }, false)
        : DialogOpenResult.CancelledResult;
}

static DialogOpenResult SaveFileDialogNative(DialogFilter[] filters)
{
    var filterList = BuildNativeFilter(filters);
    var result = Nfd.SaveDialog(out var path, filterList, null);
    return result == NfdResult.Ok && !string.IsNullOrWhiteSpace(path)
        ? new DialogOpenResult(new[] { path }, false)
        : DialogOpenResult.CancelledResult;
}

static DialogOpenResult OpenFolderDialogNative()
{
    var result = Nfd.PickFolder(out var path, null);
    return result == NfdResult.Ok && !string.IsNullOrWhiteSpace(path)
        ? new DialogOpenResult(new[] { path }, false)
        : DialogOpenResult.CancelledResult;
}
#endif

internal sealed record Request(string Id, string Command, JsonElement Payload);

internal sealed record Response(string Id, string Status, object? Payload, ErrorDetails? Error)
{
    public static Response Ok(string id, object? payload) => new(id, "ok", payload, null);

    public static Response Error(string id, string category, string message, object? details)
        => new(id, "error", null, new ErrorDetails(category, message, details));
}

internal sealed record ErrorDetails(string Category, string Message, object? Details);

internal sealed record DialogOpenRequest(
    string Kind,
    string? Title,
    DialogFilter[]? Filters,
    DialogOptions? Options);

internal sealed record DialogFilter(string Label, string[] Extensions);

internal sealed record DialogOptions(
    [property: JsonPropertyName("multi_select")] bool MultiSelect,
    [property: JsonPropertyName("show_hidden")] bool ShowHidden);

internal sealed record CredentialRequest(
    string Service,
    string? Account,
    string Action,
    string? Secret);

internal sealed record CredentialPayload(string? Username, string? Secret, string Status);

internal sealed record LibraryCallRequest(string Name, JsonElement Payload);

internal static class CredentialProviderFactory
{
    public static ICredentialProvider Create()
    {
#if WINDOWS
        if (OperatingSystem.IsWindows())
        {
            return new WindowsCredentialProvider();
        }
#endif
        if (OperatingSystem.IsMacOS())
        {
            return new MacKeychainProvider();
        }
        if (OperatingSystem.IsLinux())
        {
            return new LinuxSecretServiceProvider();
        }
        return new StubCredentialProvider();
    }
}

internal interface ICredentialProvider
{
    CredentialPayload Get(string service, string? account);
    CredentialPayload Store(string service, string? account, string? secret);
    CredentialPayload Erase(string service, string? account);
}

internal static class CredentialStatus
{
    public const string Ok = "ok";
    public const string NotFound = "not_found";
    public const string Denied = "denied";
    public const string Error = "error";
}

internal static class CredentialStatusMapper
{
#if WINDOWS
    private const int ErrorNotFound = 1168;
    private const int ErrorAccessDenied = 5;

    public static string FromWindowsError(int error)
    {
        return error switch
        {
            ErrorNotFound => CredentialStatus.NotFound,
            ErrorAccessDenied => CredentialStatus.Denied,
            _ => CredentialStatus.Error
        };
    }
#endif

    private const int ErrSecItemNotFound = -25300;
    private const int ErrSecAuthFailed = -25293;

    public static string FromMacStatus(int status)
    {
        return status switch
        {
            ErrSecItemNotFound => CredentialStatus.NotFound,
            ErrSecAuthFailed => CredentialStatus.Denied,
            _ => CredentialStatus.Error
        };
    }

    public static string FromLinuxErrorName(string errorName)
    {
        return errorName switch
        {
            "org.freedesktop.Secret.Error.NoSuchObject" => CredentialStatus.NotFound,
            "org.freedesktop.Secret.Error.IsLocked" => CredentialStatus.Denied,
            "org.freedesktop.Secret.Error.PermissionDenied" => CredentialStatus.Denied,
            _ => CredentialStatus.Error
        };
    }
}

internal sealed class StubCredentialProvider : ICredentialProvider
{
    public CredentialPayload Get(string service, string? account)
        => new(null, null, CredentialStatus.NotFound);

    public CredentialPayload Store(string service, string? account, string? secret)
        => new(null, null, CredentialStatus.Denied);

    public CredentialPayload Erase(string service, string? account)
        => new(null, null, CredentialStatus.Denied);
}

internal sealed class MacKeychainProvider : ICredentialProvider
{
    private const int ErrSecSuccess = 0;
    private const string SecurityLibrary = "/System/Library/Frameworks/Security.framework/Security";
    private const string CoreFoundationLibrary = "/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation";

    public CredentialPayload Get(string service, string? account)
    {
        var serviceName = Encoding.UTF8.GetBytes(service);
        var accountName = string.IsNullOrWhiteSpace(account) ? Array.Empty<byte>() : Encoding.UTF8.GetBytes(account);

        var status = SecKeychainFindGenericPassword(
            IntPtr.Zero,
            (uint)serviceName.Length,
            serviceName,
            (uint)accountName.Length,
            accountName.Length == 0 ? IntPtr.Zero : accountName,
            out var passwordLength,
            out var passwordData,
            out var itemRef);

        if (status != ErrSecSuccess)
        {
            return new CredentialPayload(null, null, CredentialStatusMapper.FromMacStatus(status));
        }

        try
        {
            var secret = ReadSecret(passwordData, (int)passwordLength);
            return new CredentialPayload(account, secret, CredentialStatus.Ok);
        }
        finally
        {
            if (passwordData != IntPtr.Zero)
            {
                SecKeychainItemFreeContent(IntPtr.Zero, passwordData);
            }

            if (itemRef != IntPtr.Zero)
            {
                CFRelease(itemRef);
            }
        }
    }

    public CredentialPayload Store(string service, string? account, string? secret)
    {
        if (string.IsNullOrWhiteSpace(account) || string.IsNullOrWhiteSpace(secret))
        {
            return new CredentialPayload(null, null, CredentialStatus.Denied);
        }

        var serviceName = Encoding.UTF8.GetBytes(service);
        var accountName = Encoding.UTF8.GetBytes(account);
        var passwordBytes = Encoding.UTF8.GetBytes(secret);

        var status = SecKeychainAddGenericPassword(
            IntPtr.Zero,
            (uint)serviceName.Length,
            serviceName,
            (uint)accountName.Length,
            accountName,
            (uint)passwordBytes.Length,
            passwordBytes,
            out var itemRef);

        if (itemRef != IntPtr.Zero)
        {
            CFRelease(itemRef);
        }

        return status == ErrSecSuccess
            ? new CredentialPayload(account, null, CredentialStatus.Ok)
            : new CredentialPayload(null, null, CredentialStatusMapper.FromMacStatus(status));
    }

    public CredentialPayload Erase(string service, string? account)
    {
        var serviceName = Encoding.UTF8.GetBytes(service);
        var accountName = string.IsNullOrWhiteSpace(account) ? Array.Empty<byte>() : Encoding.UTF8.GetBytes(account);

        var status = SecKeychainFindGenericPassword(
            IntPtr.Zero,
            (uint)serviceName.Length,
            serviceName,
            (uint)accountName.Length,
            accountName.Length == 0 ? IntPtr.Zero : accountName,
            out var passwordLength,
            out var passwordData,
            out var itemRef);

        if (status != ErrSecSuccess)
        {
            return new CredentialPayload(null, null, CredentialStatusMapper.FromMacStatus(status));
        }

        try
        {
            var deleteStatus = SecKeychainItemDelete(itemRef);
            return deleteStatus == ErrSecSuccess
                ? new CredentialPayload(null, null, CredentialStatus.Ok)
                : new CredentialPayload(null, null, CredentialStatusMapper.FromMacStatus(deleteStatus));
        }
        finally
        {
            if (passwordData != IntPtr.Zero)
            {
                SecKeychainItemFreeContent(IntPtr.Zero, passwordData);
            }

            if (itemRef != IntPtr.Zero)
            {
                CFRelease(itemRef);
            }
        }
    }

    private static string? ReadSecret(IntPtr passwordData, int length)
    {
        if (passwordData == IntPtr.Zero || length == 0)
        {
            return null;
        }

        var buffer = new byte[length];
        Marshal.Copy(passwordData, buffer, 0, length);
        return Encoding.UTF8.GetString(buffer);
    }

    [DllImport(SecurityLibrary)]
    private static extern int SecKeychainFindGenericPassword(
        IntPtr keychain,
        uint serviceNameLength,
        byte[] serviceName,
        uint accountNameLength,
        IntPtr accountName,
        out uint passwordLength,
        out IntPtr passwordData,
        out IntPtr itemRef);

    [DllImport(SecurityLibrary)]
    private static extern int SecKeychainFindGenericPassword(
        IntPtr keychain,
        uint serviceNameLength,
        byte[] serviceName,
        uint accountNameLength,
        byte[] accountName,
        out uint passwordLength,
        out IntPtr passwordData,
        out IntPtr itemRef);

    [DllImport(SecurityLibrary)]
    private static extern int SecKeychainAddGenericPassword(
        IntPtr keychain,
        uint serviceNameLength,
        byte[] serviceName,
        uint accountNameLength,
        byte[] accountName,
        uint passwordLength,
        byte[] passwordData,
        out IntPtr itemRef);

    [DllImport(SecurityLibrary)]
    private static extern int SecKeychainItemDelete(IntPtr itemRef);

    [DllImport(SecurityLibrary)]
    private static extern int SecKeychainItemFreeContent(IntPtr attrList, IntPtr data);

    [DllImport(CoreFoundationLibrary)]
    private static extern void CFRelease(IntPtr cfRef);
}

internal sealed class LinuxSecretServiceProvider : ICredentialProvider
{
    private const string ServiceName = "org.freedesktop.secrets";
    private static readonly ObjectPath ServicePath = new("/org/freedesktop/secrets");
    private static readonly ObjectPath RootPromptPath = new("/");

    public CredentialPayload Get(string service, string? account)
        => Execute(() => GetAsync(service, account));

    public CredentialPayload Store(string service, string? account, string? secret)
        => Execute(() => StoreAsync(service, account, secret));

    public CredentialPayload Erase(string service, string? account)
        => Execute(() => EraseAsync(service, account));

    private static CredentialPayload Execute(Func<Task<CredentialPayload>> operation)
    {
        try
        {
            return operation().GetAwaiter().GetResult();
        }
        catch (DBusException ex)
        {
            return new CredentialPayload(null, null, CredentialStatusMapper.FromLinuxErrorName(ex.ErrorName));
        }
        catch
        {
            return new CredentialPayload(null, null, CredentialStatus.Error);
        }
    }

    private static async Task<CredentialPayload> GetAsync(string service, string? account)
    {
        await using var connection = new Connection(Address.Session);
        await connection.ConnectAsync();

        var secretService = connection.CreateProxy<ISecretService>(ServiceName, ServicePath);
        var session = await OpenSessionAsync(secretService);

        var attributes = BuildAttributes(service, account);
        var (unlocked, locked) = await secretService.SearchItemsAsync(attributes);

        if (locked.Length > 0)
        {
            var (_, promptPath) = await secretService.UnlockAsync(locked);
            if (promptPath != RootPromptPath)
            {
                return new CredentialPayload(null, null, CredentialStatus.Denied);
            }
        }

        var itemPath = unlocked.FirstOrDefault();
        if (itemPath == default)
        {
            return new CredentialPayload(null, null, CredentialStatus.NotFound);
        }

        var item = connection.CreateProxy<ISecretItem>(ServiceName, itemPath);
        var secret = await item.GetSecretAsync(session);
        var username = account;
        if (string.IsNullOrWhiteSpace(username))
        {
            var itemAttributes = await item.GetAttributesAsync();
            if (itemAttributes.TryGetValue("account", out var foundAccount))
            {
                username = foundAccount;
            }
        }

        var secretValue = secret.Value.Length > 0
            ? Encoding.UTF8.GetString(secret.Value)
            : null;

        return new CredentialPayload(username, secretValue, CredentialStatus.Ok);
    }

    private static async Task<CredentialPayload> StoreAsync(string service, string? account, string? secret)
    {
        if (string.IsNullOrWhiteSpace(account) || string.IsNullOrWhiteSpace(secret))
        {
            return new CredentialPayload(null, null, CredentialStatus.Denied);
        }

        await using var connection = new Connection(Address.Session);
        await connection.ConnectAsync();

        var secretService = connection.CreateProxy<ISecretService>(ServiceName, ServicePath);
        var session = await OpenSessionAsync(secretService);

        var collectionPath = await secretService.ReadAliasAsync("default");
        if (collectionPath == default)
        {
            return new CredentialPayload(null, null, CredentialStatus.Denied);
        }

        var properties = new Dictionary<string, object>
        {
            ["org.freedesktop.Secret.Item.Label"] = $"{service}:{account}",
            ["org.freedesktop.Secret.Item.Attributes"] = BuildAttributes(service, account)
        };

        var secretStruct = new Secret(
            session,
            Array.Empty<byte>(),
            Encoding.UTF8.GetBytes(secret),
            "text/plain");

        var (_, promptPath) = await secretService.CreateItemAsync(collectionPath, properties, secretStruct, true);
        if (promptPath != RootPromptPath)
        {
            return new CredentialPayload(null, null, CredentialStatus.Denied);
        }

        return new CredentialPayload(account, null, CredentialStatus.Ok);
    }

    private static async Task<CredentialPayload> EraseAsync(string service, string? account)
    {
        await using var connection = new Connection(Address.Session);
        await connection.ConnectAsync();

        var secretService = connection.CreateProxy<ISecretService>(ServiceName, ServicePath);
        var attributes = BuildAttributes(service, account);
        var (unlocked, locked) = await secretService.SearchItemsAsync(attributes);
        var itemPath = unlocked.FirstOrDefault();

        if (itemPath == default && locked.Length > 0)
        {
            var (unlockResult, promptPath) = await secretService.UnlockAsync(locked);
            if (promptPath != RootPromptPath)
            {
                return new CredentialPayload(null, null, CredentialStatus.Denied);
            }

            itemPath = unlockResult.FirstOrDefault();
        }

        if (itemPath == default)
        {
            return new CredentialPayload(null, null, CredentialStatus.NotFound);
        }

        var item = connection.CreateProxy<ISecretItem>(ServiceName, itemPath);
        var prompt = await item.DeleteAsync();
        if (prompt != RootPromptPath)
        {
            return new CredentialPayload(null, null, CredentialStatus.Denied);
        }

        return new CredentialPayload(null, null, CredentialStatus.Ok);
    }

    private static IDictionary<string, string> BuildAttributes(string service, string? account)
    {
        var attributes = new Dictionary<string, string>
        {
            ["service"] = service
        };

        if (!string.IsNullOrWhiteSpace(account))
        {
            attributes["account"] = account;
        }

        return attributes;
    }

    private static async Task<ObjectPath> OpenSessionAsync(ISecretService secretService)
    {
        var (session, _) = await secretService.OpenSessionAsync("plain", string.Empty);
        return session;
    }

}

[DBusInterface("org.freedesktop.Secret.Service")]
internal interface ISecretService : IDBusObject
{
    Task<(ObjectPath session, object output)> OpenSessionAsync(string algorithm, object input);
    Task<(ObjectPath[] unlocked, ObjectPath[] locked)> SearchItemsAsync(IDictionary<string, string> attributes);
    Task<(ObjectPath[] unlocked, ObjectPath prompt)> UnlockAsync(ObjectPath[] items);
    Task<ObjectPath> ReadAliasAsync(string name);
    Task<(ObjectPath item, ObjectPath prompt)> CreateItemAsync(
        ObjectPath collection,
        IDictionary<string, object> properties,
        Secret secret,
        bool replace);
}

[DBusInterface("org.freedesktop.Secret.Item")]
internal interface ISecretItem : IDBusObject
{
    Task<Secret> GetSecretAsync(ObjectPath session);
    Task<ObjectPath> DeleteAsync();

    [DBusProperty("Attributes")]
    Task<IDictionary<string, string>> GetAttributesAsync();
}

[DBusStruct]
internal readonly struct Secret
{
    public ObjectPath Session { get; }
    public byte[] Parameters { get; }
    public byte[] Value { get; }
    public string ContentType { get; }

    public Secret(ObjectPath session, byte[] parameters, byte[] value, string contentType)
    {
        Session = session;
        Parameters = parameters;
        Value = value;
        ContentType = contentType;
    }
}

#if WINDOWS
internal sealed class WindowsCredentialProvider : ICredentialProvider
{
    private const uint CRED_TYPE_GENERIC = 1;
    private const uint CRED_PERSIST_LOCAL_MACHINE = 2;

    public CredentialPayload Get(string service, string? account)
    {
        if (!CredRead(service, CRED_TYPE_GENERIC, 0, out var credentialPtr))
        {
            return new CredentialPayload(null, null, CredentialStatusMapper.FromWindowsError(Marshal.GetLastWin32Error()));
        }

        try
        {
            var credential = Marshal.PtrToStructure<CREDENTIAL>(credentialPtr);
            var username = string.IsNullOrWhiteSpace(credential.UserName) ? account : credential.UserName;
            var secret = ReadCredentialBlob(credential);
            return new CredentialPayload(username, secret, CredentialStatus.Ok);
        }
        finally
        {
            CredFree(credentialPtr);
        }
    }

    public CredentialPayload Store(string service, string? account, string? secret)
    {
        if (string.IsNullOrWhiteSpace(account) || string.IsNullOrWhiteSpace(secret))
        {
            return new CredentialPayload(null, null, CredentialStatus.Denied);
        }

        var secretBytes = Encoding.UTF8.GetBytes(secret);
        var secretPtr = Marshal.AllocHGlobal(secretBytes.Length);
        try
        {
            Marshal.Copy(secretBytes, 0, secretPtr, secretBytes.Length);
            var credential = new CREDENTIAL
            {
                Type = CRED_TYPE_GENERIC,
                TargetName = service,
                UserName = account,
                CredentialBlobSize = (uint)secretBytes.Length,
                CredentialBlob = secretPtr,
                Persist = CRED_PERSIST_LOCAL_MACHINE
            };

            if (CredWrite(ref credential, 0))
            {
                return new CredentialPayload(account, null, CredentialStatus.Ok);
            }

            return new CredentialPayload(null, null, CredentialStatusMapper.FromWindowsError(Marshal.GetLastWin32Error()));
        }
        finally
        {
            Marshal.FreeHGlobal(secretPtr);
        }
    }

    public CredentialPayload Erase(string service, string? account)
    {
        if (CredDelete(service, CRED_TYPE_GENERIC, 0))
        {
            return new CredentialPayload(null, null, CredentialStatus.Ok);
        }

        return new CredentialPayload(null, null, CredentialStatusMapper.FromWindowsError(Marshal.GetLastWin32Error()));
    }

    private static string? ReadCredentialBlob(CREDENTIAL credential)
    {
        if (credential.CredentialBlob == IntPtr.Zero || credential.CredentialBlobSize == 0)
        {
            return null;
        }

        var blob = new byte[credential.CredentialBlobSize];
        Marshal.Copy(credential.CredentialBlob, blob, 0, (int)credential.CredentialBlobSize);
        return Encoding.UTF8.GetString(blob).TrimEnd('\0');
    }

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredRead(string target, uint type, uint flags, out IntPtr credentialPtr);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredWrite(ref CREDENTIAL credential, uint flags);

    [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredDelete(string target, uint type, uint flags);

    [DllImport("advapi32.dll", SetLastError = true)]
    private static extern void CredFree(IntPtr buffer);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct CREDENTIAL
    {
        public uint Flags;
        public uint Type;
        public string TargetName;
        public string Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public uint CredentialBlobSize;
        public IntPtr CredentialBlob;
        public uint Persist;
        public uint AttributeCount;
        public IntPtr Attributes;
        public string TargetAlias;
        public string UserName;
    }
}
#endif

internal sealed record DialogOpenResult(string[] Paths, bool Cancelled)
{
    public static DialogOpenResult CancelledResult { get; } = new(Array.Empty<string>(), true);
}
