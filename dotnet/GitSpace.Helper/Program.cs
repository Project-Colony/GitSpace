using Microsoft.Extensions.Logging;
using System.Text.Json;

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

var response = request.Command.Equals("ping", StringComparison.OrdinalIgnoreCase)
    ? new Response("ok", "pong")
    : new Response("error", "unknown command");

Console.WriteLine(JsonSerializer.Serialize(response));

internal sealed record Request(string Command);

internal sealed record Response(string Status, string Message);
