using Microsoft.Extensions.Logging;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;
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
    _ => Response.Error(
        request.Id,
        "InvalidRequest",
        "Unknown command",
        new { command = request.Command })
};

Console.WriteLine(JsonSerializer.Serialize(response));

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
    string Action);

internal sealed record CredentialPayload(string? Username, string? Secret, string Status);

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
            "store" => provider.Store(payload.Service, payload.Account),
            "erase" => provider.Erase(payload.Service, payload.Account),
            _ => new CredentialPayload(null, null, "denied")
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

internal interface ICredentialProvider
{
    CredentialPayload Get(string service, string? account);
    CredentialPayload Store(string service, string? account);
    CredentialPayload Erase(string service, string? account);
}

internal static class CredentialProviderFactory
{
    public static ICredentialProvider Create()
        => new StubCredentialProvider();
}

internal sealed class StubCredentialProvider : ICredentialProvider
{
    public CredentialPayload Get(string service, string? account)
        => new(null, null, "not_found");

    public CredentialPayload Store(string service, string? account)
        => new(null, null, "denied");

    public CredentialPayload Erase(string service, string? account)
        => new(null, null, "denied");
}

internal sealed record DialogOpenResult(string[] Paths, bool Cancelled)
{
    public static DialogOpenResult CancelledResult { get; } = new(Array.Empty<string>(), true);
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
