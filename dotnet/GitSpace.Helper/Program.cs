using Microsoft.Extensions.Logging;
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Serialization;

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

    return Response.Ok(request.Id, new
    {
        selected_paths = Array.Empty<string>(),
        cancelled = true
    });
}
